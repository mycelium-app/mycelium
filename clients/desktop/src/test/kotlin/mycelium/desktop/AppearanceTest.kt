package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.printToLog
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.ui.Accent
import mycelium.ui.Appearance
import mycelium.ui.Display
import mycelium.ui.Label
import mycelium.ui.LocalAppearance
import mycelium.ui.MotionSetting
import mycelium.ui.MyceliumTheme
import mycelium.ui.SettingsScreen
import mycelium.ui.ThreadView
import mycelium.ui.Tokens
import mycelium.ui.timestamp
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class AppearanceTest {
    @get:Rule
    val rule = createComposeRule()

    private val prose = "wordy ".repeat(400).trim()

    private fun show() {
        val engine = MockEngine { respond("[]", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json")) }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        rule.setContent {
            var display by androidx.compose.runtime.remember { mutableStateOf(Display()) }
            val appearance = Appearance(true, Accent.Neutral, Accent.entries, {}, display = display, onDisplay = { display = it })

            CompositionLocalProvider(LocalAppearance provides appearance) {
                MyceliumTheme(true) {
                    Column {
                        Box(Modifier.height(420.dp)) { SettingsScreen(store, Me("u", "sam@example.com", "Sam"), "https://mycelium.test", onSignOut = {}) }
                        Label("Sample code", style = Tokens.Type.Code)
                        Label("Motion is ${display.reducedMotion.name}")
                        Label("Stamped " + timestamp("2026-10-22T10:00:00Z"))
                        Box(Modifier.height(300.dp)) { ThreadView(listOf(ThreadItem.AgentMessage("m1", prose)), { _, _ -> }, {}) }
                    }
                }
            }
        }
    }

    private fun choose(option: String) {
        rule.onNodeWithText(option).performScrollTo().performClick()
    }

    private fun height(text: String): Dp = rule.onNodeWithText(text).getUnclippedBoundsInRoot().let { it.bottom - it.top }

    private fun messageWidth(): Dp = rule.onAllNodesWithText("wordy", substring = true).onFirst().getUnclippedBoundsInRoot().let { it.right - it.left }

    @Test
    fun aLargerTextSizeMakesTextTaller() {
        show()
        val before = height("Accent")

        choose("Extra large")
        rule.waitForIdle()

        assertTrue(height("Accent") > before)
    }

    @Test
    fun aCompactDensityShortensControls() {
        show()
        val before = rule.onNodeWithContentDescription("Density").getUnclippedBoundsInRoot().let { it.bottom - it.top }

        choose("Compact")
        rule.waitForIdle()

        assertTrue(rule.onNodeWithContentDescription("Density").getUnclippedBoundsInRoot().let { it.bottom - it.top } < before)
    }

    @Test
    fun theChatWidthSetsHowWideTheConversationReads() {
        show()
        val standard = messageWidth()

        choose("Narrow")
        rule.waitForIdle()
        val narrow = messageWidth()

        choose("Full")
        rule.waitForIdle()

        assertTrue(narrow < standard)
        assertTrue(messageWidth() > standard)
    }

    @Test
    fun aBiggerCodeSizeMakesCodeTaller() {
        show()
        val before = height("Sample code")

        choose("Bigger")
        rule.waitForIdle()

        assertTrue(height("Sample code") > before)
    }

    @Test
    fun theReducedMotionChoiceIsRecordedAndOverridesTheSystem() {
        show()

        rule.onNodeWithText("Motion is System").assertExists()
        choose("On")
        rule.waitForIdle()

        rule.onNodeWithText("Motion is On").assertExists()
        assertEquals(true, MotionSetting.On.reduced(system = false))
        assertEquals(false, MotionSetting.Off.reduced(system = true))
        assertEquals(true, MotionSetting.System.reduced(system = true))
    }

    @Test
    fun absoluteTimesReplaceRelativeOnes() {
        show()

        rule.onNodeWithText("Stamped 22 Oct 2026 10:00").assertDoesNotExist()

        choose("Absolute")
        rule.waitForIdle()

        rule.onNodeWithText("Stamped 22 Oct 2026 10:00").assertExists()
    }
}
