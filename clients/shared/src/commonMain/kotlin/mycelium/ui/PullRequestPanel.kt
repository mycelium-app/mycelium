package mycelium.ui

import androidx.compose.foundation.background
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
import mycelium.state.Store
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

@Composable
fun PersonAvatar(person: ForgePerson, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Box(
        modifier.size(Tokens.Size.IconMd).background(colors.bgPanel, CircleShape).semantics { contentDescription = person.name },
        contentAlignment = Alignment.Center,
    ) {
        Label(initials(person.name), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
    }
}

@Composable
fun PeopleStack(author: ForgePerson?, assignees: List<ForgePerson>) {
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        author?.let { PersonAvatar(it) }

        for (person in assignees.filter { it.name != author?.name }) {
            PersonAvatar(person)
        }
    }
}

@Composable
fun PullRequestPanel(store: Store, request: PullRequestItem, modifier: Modifier = Modifier, actions: @Composable () -> Unit = {}) {
    val colors = MyTheme.colors
    var tab by remember(request.forgeId, request.number) { mutableStateOf(PullRequestTabs.first()) }
    var onlyUnresolved by remember(request.forgeId, request.number) { mutableStateOf(false) }
    var detail by remember(request.forgeId, request.number) { mutableStateOf<PullRequestDetail?>(null) }
    var diff by remember(request.forgeId, request.number) { mutableStateOf<String?>(null) }
    var problem by remember(request.forgeId, request.number) { mutableStateOf<String?>(null) }

    LaunchedEffect(request.forgeId, request.number, tab) {
        problem = when (tab) {
            "Timeline" -> attempt { detail = store.api.pullRequest(request.forgeId, request.repository, request.number) }
            "Code" -> attempt { diff = store.api.pullRequestDiff(request.forgeId, request.repository, request.number).diff }
            else -> null
        }
    }

    Column(modifier.fillMaxSize()) {
        Column(Modifier.fillMaxWidth().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Branch, colors.textMuted, Tokens.Size.IconSm)
                Label("${request.label} ${request.title}", Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 2)
            }

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1), itemVerticalAlignment = Alignment.CenterVertically) {
                Label(pullRequestStateName(request), style = Tokens.Type.MetaStrong, color = if (request.state == PullRequestState.Open) colors.diffAdd else colors.textMuted)
                Label("${request.sourceBranch} into ${request.targetBranch}", style = Tokens.Type.Code, color = colors.textMuted, maxLines = 1)
                PeopleStack(request.author, request.assignees)
                actions()
            }
        }

        TabRow(PullRequestTabs, tab, { tab = it }, "Pull request view", Modifier.padding(horizontal = Tokens.Space.S2))
        Hairline()

        problem?.let { ErrorBanner(it, onDismiss = { problem = null }) }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            when (tab) {
                "Summary" -> Label(request.body.ifBlank { "No description." }, color = if (request.body.isBlank()) colors.textMuted else colors.text)
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
                        PullRequestCommentCard(comment)
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
private fun PullRequestCommentCard(comment: PullRequestComment) {
    val colors = MyTheme.colors

    Card {
        Column(Modifier.fillMaxWidth().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                comment.author?.let { PersonAvatar(it) }
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
