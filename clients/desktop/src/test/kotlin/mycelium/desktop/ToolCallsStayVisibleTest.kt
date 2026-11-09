package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.dp
import mycelium.state.PermissionOption
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class ToolCallsStayVisibleTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun fiveToolCallsAroundAPendingPermissionAllRenderWithNoSummaryRowAndTheCardShowsOnce() {
        val options = listOf(PermissionOption("approve", "Approve", "allow_once"), PermissionOption("deny", "Deny", "reject_once"))
        val calls = (1..5).map { ThreadItem.ToolCall("k$it", "call$it", "Read file $it", "read", "file$it.kt", ToolStatus.Done, null, null, awaitingApproval = it == 3) }
        val permission = ThreadItem.Permission("kp", "r1", "mcp__gitlab__create_merge_request", options, false, "GitLab", null, null, false, null, "call3")
        val items = calls.take(3) + permission + calls.drop(3)

        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(720.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) } } }

        (1..5).forEach { rule.onNodeWithText("file$it.kt", substring = true).assertExists() }
        assertEquals(0, rule.onAllNodesWithContentDescription("tool calls", substring = true).fetchSemanticsNodes().size)
        assertEquals(1, rule.onAllNodesWithContentDescription("Approve mcp__gitlab__create_merge_request").fetchSemanticsNodes().size)
    }
}
