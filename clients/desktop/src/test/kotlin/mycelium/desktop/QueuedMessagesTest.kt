package mycelium.desktop

import androidx.compose.ui.unit.height
import androidx.compose.ui.unit.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import mycelium.api.Attachment
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.QueuedPrompt
import mycelium.state.SessionOptions
import mycelium.ui.Composer
import mycelium.ui.LocalAttachmentBytes
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.QueueEdit
import org.junit.Rule
import kotlin.math.abs
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class QueuedMessagesTest {
    @get:Rule
    val rule = createComposeRule()

    private val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Working, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")
    private val queue = listOf(
        QueuedPrompt("q1", "c1", 1, "Open a merge request", "2026-10-22T10:00:00Z"),
        QueuedPrompt("q2", "c1", 2, "Check the Android strings", "2026-10-22T10:00:00Z"),
    )

    private fun show(
        touch: Boolean = false,
        queued: List<QueuedPrompt> = queue,
        sendNow: (String) -> Unit = {},
        edit: (QueuedPrompt) -> Unit = {},
        move: (String, Int) -> Unit = { _, _ -> },
        remove: (String) -> Unit = {},
        queueEdit: QueueEdit? = null,
        initialText: String = "",
    ) {
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalTouch provides touch, LocalAttachmentBytes provides { testPng(96, 64) }) {
                    Composer(
                        conversation = conversation, working = true, queue = queued, sending = emptyList(), onSend = {}, onStop = {},
                        onSendQueuedNow = sendNow, onEditQueued = edit, onMoveQueued = move, onRemoveQueued = remove,
                        options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                        onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                        queueEdit = queueEdit, initialText = TextFieldValue(initialText, TextRange(initialText.length)),
                    )
                }
            }
        }
    }

    private fun openActions(number: Int) {
        rule.onNodeWithContentDescription("Actions for queued message $number").performClick()
    }

    @Test
    fun theFirstQueuedMessageIsMarkedAsNext() {
        show()

        rule.onNodeWithContentDescription("Next message").assertIsDisplayed()
        assertEquals(1, rule.onAllNodesWithContentDescription("Queued").fetchSemanticsNodes().size)
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
    fun aRowMenuOffersSendNowEditMoveAndRemove() {
        val sent = mutableListOf<String>()
        val removed = mutableListOf<String>()
        val edited = mutableListOf<String>()
        show(sendNow = { sent += it }, edit = { edited += it.id }, remove = { removed += it })

        openActions(2)
        rule.onNodeWithText("Send now").performClick()
        openActions(1)
        rule.onNodeWithText("Edit").performClick()
        openActions(1)
        rule.onNodeWithText("Remove").performClick()

        assertEquals(listOf("q2"), sent)
        assertEquals(listOf("q1"), edited)
        assertEquals(listOf("q1"), removed)
    }

    @Test
    fun moveDownAsksForTheMessageToSitOnePlaceLowerAndTheFirstRowCannotMoveUp() {
        val moves = mutableListOf<Pair<String, Int>>()
        show(move = { id, index -> moves += id to index })

        openActions(1)
        assertEquals(0, rule.onAllNodesWithText("Move up").fetchSemanticsNodes().size)
        rule.onNodeWithText("Move down").performClick()
        openActions(2)
        rule.onNodeWithText("Move up").performClick()

        assertEquals(listOf("q1" to 1, "q2" to 0), moves)
    }

    @Test
    fun aQueuedMessageWithAnImageShowsItsNameInlineAndItsThumbnailWholeAtFortyEightDp() {
        val shot = Attachment("a".repeat(64), "shot.png", "image/png", 10)
        show(queued = listOf(QueuedPrompt("q1", "c1", 1, "The header overlaps the clock", "2026-10-22T10:00:00Z", listOf(shot))))

        rule.waitUntil(5_000) { rule.onAllNodesWithTag("queued-thumbnail").fetchSemanticsNodes().isNotEmpty() }
        val thumbnail = rule.onNodeWithTag("queued-thumbnail").getBoundsInRoot()

        rule.onNodeWithText("Image · shot.png", substring = true).assertIsDisplayed()
        rule.onNodeWithContentDescription("shot.png").assertIsDisplayed()
        assertEquals(48f, thumbnail.height.value, 0.5f)
        assertTrue(abs(thumbnail.width.value - 48f * 96f / 64f) <= 1f)
    }

    @Test
    fun aMessageBeingEditedShowsWhereItIsBeingEditedInsteadOfARowMenu() {
        show(queued = listOf(queue[0], queue[1].copy(editing = true)))

        rule.onNodeWithText("Editing in the composer").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Actions for queued message 2").fetchSemanticsNodes().size)
        assertEquals(1, rule.onAllNodesWithContentDescription("Actions for queued message 1").fetchSemanticsNodes().size)
    }

    @Test
    fun anEditStripNamesTheMessageHidesSendAndSavesTheFieldText() {
        val saved = mutableListOf<String>()
        show(queueEdit = QueueEdit(2, { saved += it }, {}), initialText = "Check the strings again")

        rule.onNodeWithText("Editing queued message 2").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Queue message").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Stop the agent").assertIsDisplayed()

        rule.onNodeWithText("Save").performClick()

        assertEquals(listOf("Check the strings again"), saved)
    }

    @Test
    fun escapeInTheFieldCancelsTheEdit() {
        var cancelled = 0
        show(queueEdit = QueueEdit(1, {}, { cancelled++ }), initialText = "half edited")

        rule.onNodeWithContentDescription("Message").performClick()
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Escape) }

        assertEquals(1, cancelled)
    }
}
