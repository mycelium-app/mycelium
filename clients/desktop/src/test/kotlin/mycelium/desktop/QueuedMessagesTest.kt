package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.QueuedPrompt
import mycelium.state.SessionOptions
import mycelium.ui.Composer
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class QueuedMessagesTest {
    @get:Rule
    val rule = createComposeRule()

    private val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Working, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")
    private val queue = listOf(
        QueuedPrompt("q1", "c1", 1, "Open a merge request", "2026-10-22T10:00:00Z"),
        QueuedPrompt("q2", "c1", 2, "Check the Android strings", "2026-10-22T10:00:00Z"),
    )

    private fun show(touch: Boolean = false, sendNow: (String) -> Unit = {}, edit: (String, String) -> Unit = { _, _ -> }, remove: (String) -> Unit = {}) {
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalTouch provides touch) {
                    Composer(
                        conversation = conversation, working = true, queue = queue, sending = emptyList(), onSend = {}, onStop = {},
                        onSendQueuedNow = sendNow, onEditQueued = edit, onRemoveQueued = remove,
                        options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                        onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                    )
                }
            }
        }
    }

    @Test
    fun theFirstQueuedMessageIsMarkedAsNext() {
        show()

        rule.onNodeWithContentDescription("Next message").assertIsDisplayed()
        assertEquals(1, rule.onAllNodesWithContentDescription("Queued").fetchSemanticsNodes().size)
    }

    @Test
    fun theHeaderCountsTheQueuedMessages() {
        show()

        rule.onNodeWithText("2 queued messages").assertIsDisplayed()
    }

    @Test
    fun collapsingTheQueueHidesItsMessagesAndExpandingShowsThemAgain() {
        show()

        rule.onNodeWithContentDescription("Collapse queued messages").performClick()
        assertEquals(0, rule.onAllNodesWithText("Open a merge request").fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("Expand queued messages").performClick()
        rule.onNodeWithText("Open a merge request").assertIsDisplayed()
    }

    @Test
    fun clearAllRemovesEveryQueuedMessage() {
        val removed = mutableListOf<String>()
        show(remove = { removed += it })

        rule.onNodeWithText("Clear all").performClick()

        assertEquals(listOf("q1", "q2"), removed)
    }

    @Test
    fun aRowOffersSendNowEditAndRemoveWithoutHoverOnTouch() {
        val sent = mutableListOf<String>()
        val removed = mutableListOf<String>()
        show(touch = true, sendNow = { sent += it }, remove = { removed += it })

        rule.onAllNodesWithText("Send now")[1].performClick()
        rule.onAllNodesWithContentDescription("Remove queued message")[0].performClick()

        assertEquals(listOf("q2"), sent)
        assertEquals(listOf("q1"), removed)
    }

    @Test
    fun editingAQueuedMessageSavesTheNewText() {
        val edits = mutableListOf<Pair<String, String>>()
        show(touch = true, edit = { id, text -> edits += id to text })

        rule.onAllNodesWithContentDescription("Edit queued message")[0].performClick()
        val field = rule.onAllNodes(hasSetTextAction())[0]
        field.performTextClearance()
        field.performTextInput("Open a merge request against main")
        rule.onNodeWithText("Save").performClick()

        assertEquals(listOf("q1" to "Open a merge request against main"), edits)
    }
}
