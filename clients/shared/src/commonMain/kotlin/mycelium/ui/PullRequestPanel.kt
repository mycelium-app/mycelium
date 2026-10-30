package mycelium.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.runtime.produceState
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.painter.BitmapPainter
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.layout.ContentScale
import org.jetbrains.compose.resources.decodeToImageBitmap
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import mycelium.api.ForgePerson
import mycelium.api.PullRequestComment
import mycelium.api.PullRequestDetail
import mycelium.api.PullRequestItem
import mycelium.api.PullRequestState
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.api.Conversation
import mycelium.state.ToolStatus
import mycelium.state.initials
import mycelium.state.pullRequestFileDiffs
import mycelium.state.unresolvedOnly

private val PullRequestTabs = listOf("Summary", "Timeline", "Code")

fun pullRequestStateName(request: PullRequestItem): String = when {
    request.state == PullRequestState.Open && request.draft -> "Draft"
    request.state == PullRequestState.Open -> "Open"
    request.state == PullRequestState.Merged -> "Merged"
    else -> "Closed"
}

val LocalForgeAvatar = staticCompositionLocalOf<suspend (String, String) -> ByteArray?> { { _, _ -> null } }

@Composable
fun PersonAvatar(person: ForgePerson, forgeId: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val loadAvatar = LocalForgeAvatar.current
    val picture by produceState<Painter?>(null, forgeId, person.avatarUrl) {
        value = person.avatarUrl?.let { loadAvatar(forgeId, it) }?.let { bytes -> runCatching { BitmapPainter(bytes.decodeToImageBitmap()) }.getOrNull() }
    }
    val drawn = picture

    Box(
        modifier.size(Tokens.Size.IconMd).clip(CircleShape).background(colors.bgPanel).semantics { contentDescription = person.name },
        contentAlignment = Alignment.Center,
    ) {
        if (drawn != null) {
            Image(drawn, contentDescription = null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        } else {
            Label(initials(person.name), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
        }
    }
}

@Composable
fun PeopleStack(forgeId: String, author: ForgePerson?, assignees: List<ForgePerson>) {
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        author?.let { PersonAvatar(it, forgeId) }

        for (person in assignees.filter { it.name != author?.name }) {
            PersonAvatar(person, forgeId)
        }
    }
}

@Composable
fun PullRequestPanel(store: Store, request: PullRequestItem, modifier: Modifier = Modifier, actions: @Composable () -> Unit = {}) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var tab by remember(request.forgeId, request.repository, request.number) { mutableStateOf(PullRequestTabs.first()) }
    var onlyUnresolved by remember(request.forgeId, request.repository, request.number) { mutableStateOf(false) }
    var detail by remember(request.forgeId, request.repository, request.number) { mutableStateOf<PullRequestDetail?>(null) }
    var diff by remember(request.forgeId, request.repository, request.number) { mutableStateOf<String?>(null) }
    var problem by remember(request.forgeId, request.repository, request.number) { mutableStateOf<String?>(null) }

    LaunchedEffect(request.forgeId, request.repository, request.number, tab, state.forgeRevision) {
        problem = attempt {
            detail = store.api.pullRequest(request.forgeId, request.repository, request.number)

            if (tab == "Code") {
                diff = store.api.pullRequestDiff(request.forgeId, request.repository, request.number).diff
            }
        }
    }

    val current = detail?.pullRequest ?: request

    Column(modifier.fillMaxSize()) {
        Column(Modifier.fillMaxWidth().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Branch, colors.textMuted, Tokens.Size.IconSm)
                Label("${current.label} ${current.title}", Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 2)
            }

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1), itemVerticalAlignment = Alignment.CenterVertically) {
                Label(pullRequestStateName(current), style = Tokens.Type.MetaStrong, color = if (current.state == PullRequestState.Open) colors.diffAdd else colors.textMuted)
                Label("${current.sourceBranch} into ${current.targetBranch}", style = Tokens.Type.Code, color = colors.textMuted, maxLines = 1)
                PeopleStack(request.forgeId, current.author, current.assignees)
                actions()
            }
        }

        TabRow(PullRequestTabs, tab, { tab = it }, "Pull request view", Modifier.padding(horizontal = Tokens.Space.S2))
        Hairline()

        problem?.let { ErrorBanner(it, onDismiss = { problem = null }) }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            when (tab) {
                "Summary" -> Label(current.body.ifBlank { "No description." }, color = if (current.body.isBlank()) colors.textMuted else colors.text)
                "Timeline" -> {
                    val timeline = detail?.timeline.orEmpty()
                    val shown = if (onlyUnresolved) unresolvedOnly(timeline) else timeline

                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        Checkbox(onlyUnresolved, "Show unresolved comments only", { onlyUnresolved = !onlyUnresolved }, checkedState = "On", uncheckedState = "Off")
                        Label("Unresolved only", maxLines = 1)
                    }

                    if (detail != null && shown.isEmpty()) {
                        Label(if (onlyUnresolved) "No unresolved comments." else "No comments yet.", color = colors.textMuted)
                    }

                    for (comment in shown) {
                        PullRequestCommentCard(comment, request.forgeId)
                    }
                }
                else -> {
                    val files = diff?.let(::pullRequestFileDiffs).orEmpty()

                    if (diff != null && files.isEmpty()) {
                        Label("This pull request changes no files.", color = colors.textMuted)
                    }

                    for (file in files) {
                        DiffCard(file, ToolStatus.Done)
                    }
                }
            }
        }
    }
}

@Composable
private fun PullRequestCommentCard(comment: PullRequestComment, forgeId: String) {
    val colors = MyTheme.colors

    Card {
        Column(Modifier.fillMaxWidth().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                comment.author?.let { PersonAvatar(it, forgeId) }
                Label(comment.author?.name ?: "Unknown", Modifier.weight(1f, fill = false), Tokens.Type.UiStrong, maxLines = 1)

                if (comment.resolved == true) {
                    Icon(MyIcon.Check, colors.textMuted, Tokens.Size.IconSm)
                    Label("Resolved", style = Tokens.Type.Meta, color = colors.textMuted)
                }
            }

            comment.path?.let { Label(it, style = Tokens.Type.Code, color = colors.textMuted, maxLines = 1, modifier = Modifier.horizontalScroll(rememberScrollState())) }
            Label(comment.body)
        }
    }
}

@Composable
fun ConversationPullRequestPanel(store: Store, state: AppState, conversation: Conversation, modifier: Modifier = Modifier) {
    val project = state.projects[conversation.projectId]
    val forgeId = project?.forgeId
    val repository = project?.repository
    val number = conversation.pullRequestNumber
    var request by remember(conversation.id, forgeId, repository, number) { mutableStateOf<PullRequestItem?>(null) }
    var problem by remember(conversation.id, forgeId, repository, number) { mutableStateOf<String?>(null) }

    LaunchedEffect(conversation.id, forgeId, repository, number, conversation.pullRequestState, state.forgeRevision) {
        if (forgeId != null && repository != null && number != null) {
            problem = attempt { request = store.api.pullRequest(forgeId, repository, number).pullRequest }
        }
    }

    Column(modifier) {
        problem?.let { ErrorBanner(it, onDismiss = { problem = null }) }

        request?.let { PullRequestPanel(store, it, Modifier.fillMaxSize()) }
    }
}
