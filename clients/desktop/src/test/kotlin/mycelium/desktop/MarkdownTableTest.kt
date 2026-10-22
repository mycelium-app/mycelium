package mycelium.desktop

import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import mycelium.state.ThreadItem
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class MarkdownTableTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun aTablesCellsLineUpInColumns() {
        val table = "| Check | Result |\n|---|---|\n| sha256 of every file | ok |\n| signer | 6ca54d2f |"
        rule.setContent { MyceliumTheme(true) { ThreadView(listOf(ThreadItem.AgentMessage("m1", table)), onAnswer = { _, _ -> }, onStopWorkflow = {}) } }

        val header = rule.onNodeWithText("Result", useUnmergedTree = true).getBoundsInRoot()
        val cell = rule.onNodeWithText("6ca54d2f", useUnmergedTree = true).getBoundsInRoot()

        assertEquals(header.left, cell.left)
    }
}
