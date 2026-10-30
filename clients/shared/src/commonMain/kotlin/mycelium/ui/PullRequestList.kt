package mycelium.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import mycelium.api.Conversation
import mycelium.api.PullRequestItem
import mycelium.api.PullRequestState
import mycelium.state.AppState
import mycelium.state.Store

private val PreviewMaxHeight = 640.dp

@Composable
fun PullRequestList(store: Store, state: AppState, requests: List<PullRequestItem>, previewed: PullRequestItem?, onPreview: (PullRequestItem?) -> Unit, onOpened: (Conversation) -> Unit) {
    val colors = MyTheme.colors
    var failure by remember { mutableStateOf<String?>(null) }

    failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

    Table(
        listOf(
            Col("Project", 80.dp, max = 160.dp, priority = 2),
            Col("", 24.dp, preferred = 24.dp),
            Col("", 40.dp, numeric = true, priority = 1),
            Col("Pull request", 100.dp, weight = 1f, overflow = Overflow.Wrap),
            Col("People", 60.dp, preferred = 60.dp, priority = 2),
            Col("State", 70.dp, priority = 1),
        ),
        requests.size,
        { row, column ->
            val request = requests[row]

            when (column) {
                0 -> TableCell.Text(request.repository.substringAfterLast('/'), Tokens.Type.Meta, colors.textMuted)
                1 -> TableCell.Slot { Icon(MyIcon.Branch, colors.textMuted, Tokens.Size.IconSm) }
                2 -> TableCell.Text(request.label.substringAfterLast(' '), Tokens.Type.Code, colors.textFaint)
                3 -> TableCell.Text(request.title)
                4 -> TableCell.Slot { PeopleStack(request.forgeId, request.author, request.assignees) }
                else -> TableCell.Text(pullRequestStateName(request), Tokens.Type.Meta, if (request.state == PullRequestState.Open) colors.diffAdd else colors.textMuted)
            }
        },
        lazy = false,
        onRow = { onPreview(requests[it]) },
        rowDescription = { "${requests[it].label} ${requests[it].title}, ${pullRequestStateName(requests[it])}" },
    )

    if (requests.isEmpty()) {
        Label("No pull requests.", Modifier.padding(Tokens.Space.S2), color = colors.textMuted)
    }

    previewed?.let { request ->
        val project = state.projects.values.firstOrNull { it.forgeId == request.forgeId && it.repository == request.repository }

        Dialog(onDismiss = { onPreview(null) }) {
            Box(Modifier.fillMaxWidth().heightIn(max = PreviewMaxHeight)) {
                PullRequestPanel(store, request, actions = {
                    if (project != null) {
                        MyButton(
                            {
                                val agent = state.nodes[project.nodeId]?.agents?.firstOrNull { it.available }?.agent

                                if (agent == null) {
                                    failure = "No agent is installed on that node. Install Claude Code or Codex there, then try again."
                                } else {
                                    store.act { onOpened(store.api.startPullRequestWorkspace(project.id, request.number, agent)) }
                                }

                                onPreview(null)
                            },
                            text = "Load as workspace",
                            size = ButtonSize.Sm,
                            variant = ButtonVariant.Outline,
                            description = "Load ${request.label} as a workspace",
                        )
                    }
                })
            }
        }
    }
}
