package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.state.Choice
import mycelium.state.SessionOptions
import mycelium.state.Setting
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.ui.Composer
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ReleaseBugsTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun crowdedDesktopOptionsLeaveStopAndSendVisibleAndUsable() {
        var stopped = false
        val sent = mutableListOf<String>()
        val conversation = Conversation("c1", "u", "n", "codex", "/work", "t", ConversationStatus.Working, createdAt = "2026-10-24T10:00:00Z", updatedAt = "2026-10-24T10:00:00Z")
        val choices = listOf(Choice("chosen", "A long configuration choice"))
        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(700.dp)) {
                    Composer(
                        conversation = conversation, working = true, queue = emptyList(), sending = emptyList(),
                        onSend = { sent += it }, onStop = { stopped = true },
                        onSendQueuedNow = {}, onEditQueued = {}, onRemoveQueued = {},
                        options = SessionOptions(choices, "chosen", choices, "chosen", (1..5).map { Setting("s$it", "Setting $it", null, "chosen", choices) }),
                        commands = emptyList(), onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                    )
                }
            }
        }

        val stop = rule.onNodeWithContentDescription("Stop the agent")
        val send = rule.onNodeWithContentDescription("Queue message")
        stop.assertIsDisplayed()
        send.assertIsDisplayed()
        val stopBounds = stop.getBoundsInRoot()
        val sendBounds = send.getBoundsInRoot()
        assertTrue(stopBounds.right - stopBounds.left >= 24.dp)
        assertTrue(sendBounds.right - sendBounds.left >= 24.dp)
        stop.performClick()
        assertTrue(stopped)
    }

    @Test
    fun guardianReviewIsRenderedAsAReviewWithItsDecisionRatherThanARawThinkCommand() {
        val review = ThreadItem.ToolCall("review", "review", "Guardian Review", "think", "Guardian Review", ToolStatus.Done, null, "Status: Approved")
        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(600.dp)) {
                    ThreadView(listOf(review), onAnswer = { _, _ -> }, onStopWorkflow = {}, agent = "codex")
                }
            }
        }

        rule.onNodeWithText("Guardian Review").assertIsDisplayed()
        rule.onNodeWithText("Approved").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("think").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Status: Approved").fetchSemanticsNodes().size)
    }
}
