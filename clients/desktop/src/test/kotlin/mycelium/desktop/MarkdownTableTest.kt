package mycelium.desktop

import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.unit.dp
import mycelium.state.ThreadItem
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

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

    @Test
    fun aTableWhoseTextWrapsUsesTheFullWidthOfTheMessage() {
        val prose = "The merge conflicted in five files, they are resolved, the checks are running in the background and nothing is committed yet, so wait for the report."
        val long = "Keeps the outcome and progress version with no log observer, plus the new finish rule for complete or warn and the cancelled or failed title, and errors come from the outcome."
        val message = "$prose\n\n| File | Resolution |\n|---|---|\n| restore.svelte | $long |"
        rule.setContent { MyceliumTheme(true) { ThreadView(listOf(ThreadItem.AgentMessage("m1", message)), onAnswer = { _, _ -> }, onStopWorkflow = {}) } }

        val paragraph = rule.onNodeWithText(prose, useUnmergedTree = true).getBoundsInRoot()
        val cell = rule.onNodeWithText(long, useUnmergedTree = true).getBoundsInRoot()

        assertTrue(paragraph.right - cell.right < 40.dp, "the table stops ${paragraph.right - cell.right} short of the message's right edge")
    }

    @Test
    fun aShortTableIsNarrowerThanTheThread() {
        val table = "| Check | Result |\n|---|---|\n| sig | ok |\n| sha | ok |"
        val prose = "A long enough paragraph that the thread is clearly wider than a two column table of short words."
        val message = "$prose\n\n$table"
        rule.setContent { MyceliumTheme(true) { ThreadView(listOf(ThreadItem.AgentMessage("m1", message)), onAnswer = { _, _ -> }, onStopWorkflow = {}) } }

        val paragraph = rule.onNodeWithText(prose, useUnmergedTree = true).getBoundsInRoot()
        val header = rule.onNodeWithText("Check", useUnmergedTree = true).getBoundsInRoot()
        val result = rule.onNodeWithText("Result", useUnmergedTree = true).getBoundsInRoot()

        assertTrue(result.right < paragraph.right - 40.dp, "the table reaches ${result.right} of ${paragraph.right}")
        assertTrue(header.left < result.left)
    }
}
