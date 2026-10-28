package mycelium.desktop

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotFocused
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performMouseInput
import mycelium.App
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.foundation.layout.width
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.performSemanticsAction
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.ui.MyButton
import mycelium.ui.MyceliumTheme
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.test.captureToImage
import mycelium.ui.SignInScreen
import org.junit.Rule
import kotlin.math.abs
import kotlin.test.Test
import kotlin.test.assertTrue

private class StubPlatform(override val signsInThroughTheBrowser: Boolean) : Platform {
    override val originUrl: String? = null
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

class AppInteractionTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun aMouseClickOnAButtonRunsIt() {
        rule.setContent { App(StubPlatform(signsInThroughTheBrowser = false), dark = true) }

        rule.onNodeWithText("Continue").performMouseInput { click() }

        rule.onNodeWithText("Enter the address of your control plane", substring = true).assertExists()
    }

    @Test
    fun aTapOnAButtonRunsIt() {
        rule.setContent { App(StubPlatform(signsInThroughTheBrowser = false), dark = true) }

        rule.onNodeWithText("Continue").performClick()

        rule.onNodeWithText("Enter the address of your control plane", substring = true).assertExists()
    }


    @Test
    fun aTappedAddressFieldStaysFocusedAndTakesText() {
        rule.setContent { App(StubPlatform(signsInThroughTheBrowser = false), dark = true) }

        rule.onNode(hasSetTextAction()).performClick()
        rule.onNode(hasSetTextAction()).assertIsFocused()
        rule.onNode(hasSetTextAction()).performTextInput("mycelium.example.com")

        rule.onNode(hasSetTextAction()).assertIsFocused().assertTextContains("mycelium.example.com")
    }

    @Test
    fun aMouseClickOnEmptySpaceClearsFocus() {
        rule.setContent { App(StubPlatform(signsInThroughTheBrowser = false), dark = true) }

        rule.onNode(hasSetTextAction()).performMouseInput { click() }
        rule.onNode(hasSetTextAction()).assertIsFocused()
        rule.onRoot().performMouseInput { click(Offset(5f, 5f)) }

        rule.onNode(hasSetTextAction()).assertIsNotFocused()
    }

    @Test
    fun aMenuItemNameIsNeverCutOffByItsDescription() {
        rule.setContent {
            MyceliumTheme(true) {
                androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.width(280.dp)) {
                    mycelium.ui.MenuItem("Accept edits", onClick = {}, hint = "Automatically accept all file edits and keep going")
                }
            }
        }

        val layouts = mutableListOf<androidx.compose.ui.text.TextLayoutResult>()
        rule.onNodeWithText("Accept edits").performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.GetTextLayoutResult) { it(layouts) }

        val layout = layouts.single()
        assertTrue(layout.multiParagraph.maxIntrinsicWidth <= layout.size.width + 0.5f && !layout.isLineEllipsized(0), "the name was cut off: width ${layout.size.width}, needs ${layout.multiParagraph.maxIntrinsicWidth}")
    }

    @Test
    fun escapeInTheComposerStopsAWorkingAgent() {
        var stopped = 0
        val conversation = mycelium.api.Conversation("c1", "u", "n", "claude", "/work", "t", mycelium.api.ConversationStatus.Working, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")
        rule.setContent {
            MyceliumTheme(true) {
                mycelium.ui.Composer(
                    conversation = conversation, working = true, queue = emptyList(), sending = emptyList(), onSend = {}, onStop = { stopped++ },
                    onSendQueuedNow = {}, onEditQueued = {}, onRemoveQueued = {},
                    options = mycelium.state.SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                )
            }
        }

        rule.onNode(hasSetTextAction()).performClick()
        rule.onNode(hasSetTextAction()).performKeyInput { pressKey(androidx.compose.ui.input.key.Key.Escape) }

        kotlin.test.assertEquals(1, stopped)
    }
}
