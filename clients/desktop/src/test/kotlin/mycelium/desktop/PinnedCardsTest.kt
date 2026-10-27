package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.WorkflowAgent
import mycelium.state.WorkflowProgress
import kotlin.time.Clock
import androidx.compose.runtime.CompositionLocalProvider
import kotlin.test.assertEquals
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test

private fun pinnedReply(heading: String, id: String = "plan") =
    "Here.\n\n```mycelium-ui\n{\"type\":\"column\",\"id\":\"$id\",\"sticky\":true,\"children\":[{\"type\":\"heading\",\"text\":\"$heading\"},{\"type\":\"text\",\"text\":\"Body of $heading\"}]}\n```"

class PinnedCardsTest {
    @get:Rule
    val rule = createComposeRule()

    private var items by mutableStateOf<List<ThreadItem>>(emptyList())
    private var dismissed by mutableStateOf(emptySet<String>())

    private fun show(width: Int = 1800) {
        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(width.dp).height(700.dp)) {
                    ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}, dismissedPins = dismissed, onUnpin = { dismissed = dismissed + it })
                }
            }
        }
    }

    @Test
    fun aStickyBlockIsShownPinnedAndUnpinRemovesIt() {
        items = listOf(ThreadItem.AgentMessage("a", pinnedReply("Rollout")))
        show()

        rule.onNodeWithContentDescription("Unpin Rollout").assertIsDisplayed()

        rule.onNodeWithContentDescription("Unpin Rollout").performClick()

        rule.waitUntil { rule.onAllNodesWithContentDescription("Unpin Rollout").fetchSemanticsNodes().isEmpty() }
        rule.onNodeWithText("Body of Rollout").assertIsDisplayed()
    }

    @Test
    fun aSecondBlockWithTheSameIdReplacesThePinnedOne() {
        items = listOf(ThreadItem.AgentMessage("a", pinnedReply("Rollout")), ThreadItem.AgentMessage("b", pinnedReply("Rollout done")))
        show()

        rule.onNodeWithContentDescription("Unpin Rollout done").assertIsDisplayed()
        rule.onNodeWithText("Body of Rollout done").assertIsDisplayed()
        rule.waitUntil { rule.onAllNodesWithContentDescription("Unpin Rollout").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun aNarrowWindowStacksPinnedCardsAboveTheComposerAndTheyCollapse() {
        items = listOf(ThreadItem.AgentMessage("a", pinnedReply("Rollout")))
        show(width = 700)

        rule.onNodeWithText("Body of Rollout").assertIsDisplayed()

        rule.onNodeWithContentDescription("Collapse Rollout").performClick()

        rule.waitUntil { rule.onAllNodesWithText("Body of Rollout").fetchSemanticsNodes().isEmpty() }
        rule.onNodeWithContentDescription("Expand Rollout").assertIsDisplayed()
    }

    @Test
    fun aRunningWorkflowIsPinnedAndItsToolCallLineIsNotDrawn() {
        val progress = WorkflowProgress("run", listOf(WorkflowAgent("a", "Survey", "Build", finished = false, tokens = 2_500, turns = 3, elapsedMs = 42_000)), lastLineAt = Clock.System.now())
        items = listOf(
            ThreadItem.ToolCall("t", "call", "Workflow", "other", "Workflow", ToolStatus.Running, null, null),
            ThreadItem.Workflow("w", progress),
        )
        show()

        rule.onNodeWithContentDescription("Unpin Workflow").assertIsDisplayed()
        rule.onNodeWithText("Survey").assertIsDisplayed()
        rule.waitUntil { rule.onAllNodesWithText("workflow").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun aWorkflowAgentWithAConversationOpensItWhenClicked() {
        val opened = mutableListOf<String>()
        items = listOf(ThreadItem.Workflow("w", WorkflowProgress("run", listOf(WorkflowAgent("k1", "write:docs", "Write", finished = true, agentId = "a1"), WorkflowAgent("k2", "write:code", "Write", finished = false, agentId = "a2")))))
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(mycelium.ui.LocalAgentOpener provides { agentId -> if (agentId == "a1") { { opened += agentId } } else null }) {
                    ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {})
                }
            }
        }

        rule.onNodeWithContentDescription("Open write:docs").performClick()

        assertEquals(listOf("a1"), opened)
        assertEquals(0, rule.onAllNodesWithContentDescription("Open write:code").fetchSemanticsNodes().size)
    }
}
