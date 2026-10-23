package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import mycelium.state.ThreadItem
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class SuggestedRepliesTest {
    @get:Rule
    val rule = createComposeRule()

    private val finished = listOf(
        ThreadItem.UserMessage("e1", "Move the restore copy"),
        ThreadItem.AgentMessage("e2", "Moved the strings. Which next?\n\n1. Open a merge request\n2. Run the e2e suite"),
        ThreadItem.TurnEnd("e3", "end_turn"),
    )

    @Test
    fun tappingASuggestedReplySendsIt() {
        val sent = mutableListOf<String>()
        rule.setContent { MyceliumTheme(true) { ThreadView(finished, onAnswer = { _, _ -> }, onStopWorkflow = {}, onSuggest = { sent += it }) } }

        rule.onNodeWithContentDescription("Reply: Run the e2e suite").assertIsDisplayed().performClick()

        assertEquals(listOf("Run the e2e suite"), sent)
    }

    @Test
    fun repliesShowOnlyUnderTheNewestFinishedTurn() {
        val older = finished + listOf(ThreadItem.UserMessage("e4", "Run the e2e suite"), ThreadItem.AgentMessage("e5", "Ran them. Want me to commit?"), ThreadItem.TurnEnd("e6", "end_turn"))
        rule.setContent { MyceliumTheme(true) { ThreadView(older, onAnswer = { _, _ -> }, onStopWorkflow = {}) } }

        assertEquals(0, rule.onAllNodesWithContentDescription("Reply: Open a merge request").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Reply: Yes").assertIsDisplayed()
    }

    @Test
    fun repliesGoOnceAMessageIsSentAfterTheTurn() {
        val sending = finished + ThreadItem.UserMessage("sending", "Open a merge request")
        rule.setContent { MyceliumTheme(true) { ThreadView(sending, onAnswer = { _, _ -> }, onStopWorkflow = {}) } }

        assertEquals(0, rule.onAllNodesWithContentDescription("Reply: Open a merge request").fetchSemanticsNodes().size)
    }
}
