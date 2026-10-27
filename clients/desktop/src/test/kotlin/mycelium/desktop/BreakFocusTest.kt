package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import mycelium.api.Me
import mycelium.ui.MyceliumTheme
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test

@OptIn(ExperimentalTestApi::class)
class BreakFocusTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    @Test
    fun duringABreakTheKeyboardStaysOnTheBreakScreen() {
        val platform = HoursPlatform("all")
        val store = HoursServer(listOf(hoursGroupJson("g-personal", "personal")), listOf(hoursConversationJson("c1", "Personal chat", "g-personal", "working"))).store(scopes.create(), platform)
        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Personal chat").fetchSemanticsNodes().isNotEmpty() }

        store.wellness.startBreak()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("End break early").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("End break early").assertIsFocused()
        repeat(3) { rule.onRoot().performKeyInput { pressKey(Key.Tab) } }
        rule.onNodeWithText("End break early").assertIsFocused()
        rule.onAllNodesWithText("Personal chat").fetchSemanticsNodes().let { assert(it.isEmpty()) { "the sidebar is hidden from assistive technology during a break" } }
    }
}
