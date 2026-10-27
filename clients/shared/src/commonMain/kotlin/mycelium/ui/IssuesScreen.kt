package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.state.ColumnAlign
import mycelium.api.Conversation
import mycelium.api.Issue
import mycelium.api.Issues
import mycelium.api.Project
import mycelium.api.PullRequestState
import mycelium.api.Worktree
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.issueCounts
import mycelium.state.projectsOfIssue
import mycelium.state.pullRequestText
import mycelium.state.worktreesOfIssue

private const val IssuesList = "issues"

@Composable
fun IssuesScreen(store: Store, state: AppState, onOpened: (Conversation) -> Unit) {
    val colors = MyTheme.colors
    var draft by remember { mutableStateOf("") }
    var query by remember { mutableStateOf("") }
    var listing by remember { mutableStateOf<Issues?>(null) }
    var worktrees by remember { mutableStateOf<List<Worktree>>(emptyList()) }
    var chosen by remember { mutableStateOf<Pair<String, String>?>(null) }
    var menuFor by remember { mutableStateOf<Long?>(null) }
    var starting by remember { mutableStateOf<Set<Long>>(emptySet()) }
    var dismissed by remember { mutableStateOf<Set<String>>(emptySet()) }
    var failure by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        store.load { store.refreshProjects() }
    }

    LaunchedEffect(Unit) {
        store.act { worktrees = store.api.worktrees() }
    }

    val loadFailures by store.loadFailures.collectAsState()
    val load = { store.load(IssuesList) { listing = store.api.issues(query) } }

    LaunchedEffect(query) {
        load()
    }

    val issues = listing?.issues.orEmpty()
    val shown = issues.filter { chosen == null || (it.forgeId to it.repository) == chosen }

    fun start(project: Project, issue: Issue) {
        val node = state.nodes[project.nodeId]
        val agent = node?.agents?.firstOrNull { it.available }?.agent

        if (agent == null) {
            failure = "No agent is installed on ${node?.name ?: "that node"}. Install Claude Code or Codex there, then try again."
            return
        }

        failure = null
        starting = starting + issue.number
        store.act {
            try {
                onOpened(store.api.startWorkspace(project.id, issue.number, agent))
            } finally {
                starting = starting - issue.number
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Issues")
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        for (problem in listing?.failures.orEmpty().filter { it.forgeId !in dismissed }) {
            ErrorBanner(problem.message, onDismiss = { dismissed = dismissed + problem.forgeId })
        }

        FlowRow(
            Modifier.fillMaxWidth().padding(Tokens.Space.S3),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            ListSearch(draft, { draft = it }, "issues", placeholder = "is:open assignee:@me", mono = true, onSubmit = { query = draft })

            for (entry in issueCounts(issues)) {
                val key = entry.forgeId to entry.repository
                val name = entry.repository.substringAfterLast('/')

                MyButton(
                    { chosen = if (chosen == key) null else key },
                    text = "$name ${entry.count}",
                    description = "Show only ${entry.repository}, ${entry.count} issues",
                    variant = ButtonVariant.Outline,
                    size = ButtonSize.Sm,
                    muted = chosen != key,
                    selected = chosen == key,
                )
            }
        }

        Hairline()

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4)) {
            if (state.projects.values.none { it.forgeId != null }) {
                Label("Issues come from forge repositories you have set up as projects. Set one up in Projects first.", Modifier.padding(Tokens.Space.S3), color = colors.textMuted)
            } else {
                Table(
                    listOf(
                        Col("", 40.dp, numeric = true, priority = 1),
                        Col("Issue", 100.dp, weight = 1f, overflow = Overflow.Wrap),
                        Col("Worktree", 120.dp, max = 240.dp, priority = 1),
                        Col("Pull request", 120.dp, max = 280.dp, priority = 1),
                        Col("", 150.dp, preferred = 150.dp, align = ColumnAlign.End),
                    ),
                    shown.size,
                    { row, column ->
                        val issue = shown[row]
                        val projects = projectsOfIssue(state.projects.values, issue)
                        val existing = worktreesOfIssue(state.projects.values, worktrees, issue)

                        when (column) {
                            0 -> TableCell.Text("#${issue.number}", Tokens.Type.Code, colors.textFaint)
                            1 -> TableCell.Text(issue.title)
                            2 -> TableCell.Slot(existing.maxByOrNull { it.branch.length }?.branch ?: "None", Tokens.Size.IconSm * 2 + Tokens.Space.S1_5 * 2) {
                                Column {
                                    if (existing.isEmpty()) {
                                        Label("None", color = colors.textFaint)
                                    }

                                    for (worktree in existing) {
                                        val node = state.nodes[state.projects[worktree.projectId]?.nodeId]

                                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                            Icon(MyIcon.Branch, colors.textMuted, Tokens.Size.IconSm)
                                            Label(worktree.branch, Modifier.weight(1f, fill = false), Tokens.Type.Code, maxLines = 1)

                                            if (node != null) {
                                                Icon(nodeIcon(node), nodeColour(node), Tokens.Size.IconSm)
                                            }
                                        }
                                    }
                                }
                            }

                            3 -> TableCell.Slot(issue.pullRequests.orEmpty().maxOfOrNull { pullRequestText(it) } ?: "Unknown") {
                                Column {
                                    val requests = issue.pullRequests

                                    when {
                                        requests == null -> Label("Unknown", color = colors.textFaint)
                                        requests.isEmpty() -> Label("None", color = colors.textFaint)
                                    }

                                    for (request in requests.orEmpty()) {
                                        Label(pullRequestText(request), color = if (request.state == PullRequestState.Open) colors.diffAdd else colors.textMuted, maxLines = 1)
                                    }
                                }
                            }

                            else -> TableCell.Slot {
                                if (issue.number in starting) {
                                    Box(Modifier.width(Tokens.Size.NumberField)) {
                                        IndeterminateProgress("Starting a workspace for issue ${issue.number}")
                                    }
                                } else if (existing.isNotEmpty()) {
                                    MyButton(
                                        { start(projects.first { it.id == existing.first().projectId }, issue) },
                                        text = "Open workspace",
                                        size = ButtonSize.Sm,
                                        description = "Open the workspace for issue ${issue.number}",
                                    )
                                } else {
                                    Box {
                                        MyButton(
                                            { if (projects.size == 1) start(projects.first(), issue) else menuFor = issue.number },
                                            text = "Create workspace",
                                            size = ButtonSize.Sm,
                                            variant = ButtonVariant.Outline,
                                            description = "Create a workspace for issue ${issue.number}",
                                        )

                                        if (menuFor == issue.number) {
                                            MenuPopup(onDismiss = { menuFor = null }) {
                                                MenuLabel("Create on")

                                                for (project in projects) {
                                                    val node = state.nodes[project.nodeId]

                                                    MenuItem(
                                                        node?.name ?: project.name,
                                                        onClick = { menuFor = null; start(project, issue) },
                                                        leading = node?.let(::nodeIcon),
                                                    )
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    lazy = false,
                )
            }

            if (listing == null && IssuesList in loadFailures) {
                LoadFailedState("issues", load, Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S4))
            } else if (listing != null && shown.isEmpty() && state.projects.values.any { it.forgeId != null }) {
                Label("No open issues match.", Modifier.padding(Tokens.Space.S2), color = colors.textMuted)
            }
        }
    }
}
