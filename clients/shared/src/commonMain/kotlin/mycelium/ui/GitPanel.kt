package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.Conversation
import mycelium.api.GitCommit
import mycelium.api.GitDiff
import mycelium.api.GitFile
import mycelium.api.GitStatus
import mycelium.state.ChangeRow
import mycelium.state.Store
import mycelium.state.ToolStatus
import mycelium.state.ago
import mycelium.state.changeRows
import mycelium.state.commitProblem
import mycelium.state.diffIsStaged
import mycelium.state.firstChange
import mycelium.state.fullyStaged
import mycelium.state.parseUnifiedDiff

private enum class GitTab { Changes, History }

private const val ShortHashLength = 7

@Composable
fun GitPanel(store: Store, conversation: Conversation, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    val id = conversation.id
    var status by remember(id) { mutableStateOf<GitStatus?>(null) }
    var log by remember(id) { mutableStateOf<List<GitCommit>>(emptyList()) }
    var tab by remember(id) { mutableStateOf(GitTab.Changes) }
    var viewing by remember(id) { mutableStateOf<GitFile?>(null) }
    var diff by remember(id) { mutableStateOf<GitDiff?>(null) }
    var message by remember(id) { mutableStateOf("") }
    var failure by remember(id) { mutableStateOf<String?>(null) }
    val files = status?.files.orEmpty()

    suspend fun refresh() {
        status = store.api.gitStatus(id)
        log = store.api.gitLog(id)
    }

    fun run(block: suspend () -> Unit) {
        scope.launch {
            try {
                block()
                failure = null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: ApiException) {
                failure = problem.message
            } catch (_: Exception) {
                failure = "Could not reach the control plane. Check your connection and try again."
            }
        }
    }

    LaunchedEffect(id, conversation.status) {
        run { refresh() }
    }

    Column(modifier.background(colors.bgPanel).semantics { contentDescription = "Git" }) {
        Bar {
            val open = viewing

            if (open != null) {
                MyButton({ viewing = null; diff = null }, icon = MyIcon.Back, description = "Back to the changes", muted = true)
                Label(open.path, Modifier.weight(1f), Tokens.Type.Code, maxLines = 1)
            } else {
                BarTitle("Git")
            }
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        val open = viewing
        val shown = diff

        if (open != null) {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
                if (shown == null) {
                    Label("Loading the diff", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
                } else if (shown.diff.isBlank()) {
                    Label("There are no text changes to show.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
                } else {
                    DiffCard(parseUnifiedDiff(open.path, shown.diff), ToolStatus.Done)

                    if (shown.truncated) {
                        Label("The diff is cut at 200 KB.", Modifier.padding(Tokens.Space.S3), Tokens.Type.Meta, colors.textMuted)
                    }
                }
            }
        } else {
            Segmented(
                listOf("Changes (${files.size})", "History"),
                if (tab == GitTab.Changes) "Changes (${files.size})" else "History",
                { tab = if (it == "History") GitTab.History else GitTab.Changes },
                "Git view",
                Modifier.padding(Tokens.Space.S2),
            )

            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())) {
                if (tab == GitTab.Changes) {
                    ChangesList(
                        files = files,
                        onViewDiff = { file ->
                            viewing = file
                            diff = null
                            run { diff = store.api.gitDiff(id, file.path, diffIsStaged(file)) }
                        },
                        onStageAll = { run { store.api.gitStageAll(id); refresh() } },
                        onToggle = { file ->
                            run {
                                if (fullyStaged(file)) store.api.gitUnstage(id, listOf(file.path)) else store.api.gitStage(id, listOf(file.path))
                                refresh()
                            }
                        },
                    )
                } else {
                    History(log)
                }
            }

            Hairline()

            CommitArea(
                status = status,
                lastCommit = log.firstOrNull(),
                message = message,
                onMessage = { message = it },
                onFetch = { run { store.api.gitFetch(id); refresh() } },
                onCommit = {
                    val problem = commitProblem(message, files)

                    if (problem != null) {
                        failure = problem
                    } else {
                        run { store.api.gitCommit(id, message.trim()); message = ""; refresh() }
                    }
                },
                onUndo = { run { store.api.gitUndo(id); refresh() } },
            )
        }
    }
}

@Composable
private fun ChangesList(files: List<GitFile>, onViewDiff: (GitFile) -> Unit, onStageAll: () -> Unit, onToggle: (GitFile) -> Unit) {
    val colors = MyTheme.colors
    val additions = files.sumOf { it.additions }
    val deletions = files.sumOf { it.deletions }
    val first = firstChange(files)

    Row(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            MyButton({ first?.let(onViewDiff) }, text = "View diff", icon = MyIcon.Diff, size = ButtonSize.Sm)
            Label("+$additions", style = Tokens.Type.Code, color = colors.diffAdd)
            Label("−$deletions", style = Tokens.Type.Code, color = colors.diffDel)
        }

        MyButton(onStageAll, text = "Stage all", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
    }

    if (files.isEmpty()) {
        Label("There are no changes in this folder.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
    }

    for (row in changeRows(files)) {
        when (row) {
            is ChangeRow.Heading -> SectionHead(row.name)
            is ChangeRow.Folder -> Row(
                Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S3),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(MyIcon.Folder, colors.textMuted, Tokens.Size.IconSm)
                Label(row.path, style = Tokens.Type.Ui, color = colors.textMuted, maxLines = 1)
            }
            is ChangeRow.Change -> ChangeItem(row, onOpen = { onViewDiff(row.file) }, onToggle = { onToggle(row.file) })
        }
    }
}

@Composable
private fun ChangeItem(row: ChangeRow.Change, onOpen: () -> Unit, onToggle: () -> Unit) {
    val colors = MyTheme.colors
    val file = row.file
    val staged = fullyStaged(file)

    Row(
        Modifier.fillMaxWidth().padding(start = if (row.indented) Tokens.Space.S6 else Tokens.Space.S3, end = Tokens.Space.S1),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier
                .weight(1f)
                .heightIn(min = rowHeight())
                .pressable(onClick = onOpen, description = "Show the changes to ${file.path}", shape = ControlShape)
                .padding(horizontal = Tokens.Space.S1),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(MyIcon.File, colors.textMuted, Tokens.Size.IconSm)
            Label(row.name, Modifier.weight(1f), maxLines = 1)

            if (file.additions > 0) {
                Label("+${file.additions}", style = Tokens.Type.Code, color = colors.diffAdd)
            }

            if (file.deletions > 0) {
                Label("−${file.deletions}", style = Tokens.Type.Code, color = colors.diffDel)
            }
        }

        Checkbox(checked = staged, description = if (staged) "Unstage ${file.path}" else "Stage ${file.path}", onToggle = onToggle)
    }
}

@Composable
fun Checkbox(checked: Boolean, description: String, onToggle: () -> Unit, checkedState: String = "Staged", uncheckedState: String = "Not staged") {
    val colors = MyTheme.colors
    val target = touchMin(Tokens.Size.ControlMd)

    Box(
        Modifier
            .size(target)
            .pressable(onClick = onToggle, role = Role.Checkbox, description = description, stateDescription = if (checked) checkedState else uncheckedState, shape = ControlShape),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(Tokens.Size.IconMd)
                .background(if (checked) colors.accent else colors.bgContent, ControlShape)
                .border(Tokens.Size.Hairline, if (checked) colors.accent else colors.borderControl, ControlShape),
            contentAlignment = Alignment.Center,
        ) {
            if (checked) {
                Icon(MyIcon.Check, colors.onAccent, Tokens.Size.IconSm)
            }
        }
    }
}

@Composable
private fun History(log: List<GitCommit>) {
    val colors = MyTheme.colors

    if (log.isEmpty()) {
        Label("There are no commits yet.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
    }

    for (commit in log) {
        Column(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1_5)
                .semantics(mergeDescendants = true) { contentDescription = "${commit.subject}, ${commit.author}, ${ago(commit.date)}" },
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
        ) {
            Label(commit.subject, maxLines = 2)

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Person, colors.textMuted, Tokens.Size.IconSm)
                Label(commit.author, Modifier.weight(1f, fill = false), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                Label(ago(commit.date), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                Label(commit.hash.take(ShortHashLength), style = Tokens.Type.CodeSm, color = colors.textFaint, maxLines = 1)
            }
        }
    }
}

@Composable
private fun CommitArea(
    status: GitStatus?,
    lastCommit: GitCommit?,
    message: String,
    onMessage: (String) -> Unit,
    onFetch: () -> Unit,
    onCommit: () -> Unit,
    onUndo: () -> Unit,
) {
    val colors = MyTheme.colors
    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()

    Column(Modifier.fillMaxWidth().padding(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(MyIcon.Branch, colors.textMuted, Tokens.Size.IconSm)
            Label(status?.branch.orEmpty(), Modifier.weight(1f, fill = false), Tokens.Type.UiStrong, maxLines = 1)

            if (status != null && status.ahead > 0) {
                Label("${status.ahead} ahead", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            if (status != null && status.behind > 0) {
                Label("${status.behind} behind", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            Box(Modifier.weight(1f))
            MyButton(onFetch, text = "Fetch", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        BasicTextField(
            value = message,
            onValueChange = onMessage,
            textStyle = Tokens.Type.Code.themed().copy(color = colors.text),
            cursorBrush = SolidColor(colors.text),
            interactionSource = source,
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 72.dp)
                .focusRing(focused, ControlShape, always = true)
                .background(colors.bgContent, ControlShape)
                .border(Tokens.Size.Hairline, colors.borderControl, ControlShape)
                .semantics { contentDescription = "Commit message" },
            decorationBox = { inner ->
                Box(Modifier.padding(Tokens.Space.S2)) {
                    if (message.isEmpty()) {
                        Label("Enter commit message", style = Tokens.Type.Code, color = colors.textFaint)
                    }

                    inner()
                }
            },
        )

        MyButton(onCommit, Modifier.fillMaxWidth(), text = "Commit", variant = ButtonVariant.Primary)

        if (lastCommit != null) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label(lastCommit.subject, Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                MyButton(onUndo, text = "Undo", description = "Undo the last commit", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }
        }
    }
}
