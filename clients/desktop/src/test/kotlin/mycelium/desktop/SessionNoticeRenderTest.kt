package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import org.junit.Rule
import mycelium.ui.MyceliumTheme
import mycelium.state.ThreadItem
import mycelium.ui.MyTheme
import mycelium.ui.ThreadView
import kotlin.test.Test
import kotlin.test.assertTrue

class SessionNoticeRenderTest {
    private val notice = "The agent could not switch to the model opus-legacy: Unknown model. Pick another model."
    private val items = listOf(
        ThreadItem.UserMessage("e1", "Tidy the release notes"),
        ThreadItem.AgentMessage("e2", "Done. I reordered the sections."),
        ThreadItem.ModelChanged("e3", "Anthropic", "opus-legacy"),
        ThreadItem.Notice("e4", notice),
    )

    @get:Rule
    val rule = createComposeRule()

    @Composable
    private fun Thread() {
        Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) {
            ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}, agent = "claude", runsOnModel = false)
        }
    }

    @Test
    fun theNoticeIsAnnouncedWithItsMessage() {
        rule.setContent { MyceliumTheme(true) { Thread() } }

        rule.onNodeWithContentDescription("Notice: $notice").assertExists()
    }

    @Test
    fun theNoticeShowsBelowTheModelSwitch() {
        val file = renderAt("session-notice", true, false) { Thread() }

        assertTrue(file.length() > 5_000)
    }
}
