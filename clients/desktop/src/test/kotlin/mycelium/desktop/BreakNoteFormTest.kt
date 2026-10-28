package mycelium.desktop

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import mycelium.api.Me
import mycelium.state.BreakPhase
import mycelium.ui.MyceliumTheme
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalTestApi::class)
class BreakNoteFormTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val field = "What did you do in your break?"

    @Test
    fun anEndedBreakShowsTheFormInsteadOfTheShellUntilANoteIsGiven() {
        val platform = HoursPlatform("all")
        val server = HoursServer(listOf(hoursGroupJson("g-personal", "personal")), listOf(hoursConversationJson("c1", "Personal chat", "g-personal", "working")))
        server.breakAwaitingNote = true
        val store = server.store(scopes.create(), platform)
        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }

        rule.onNodeWithText("Back to work").assertIsDisplayed()
        rule.onNodeWithText("For example, made a tea and stood in the garden. Only you and the Wellness assistant see this.").assertIsDisplayed()

        rule.onNodeWithText("Back to work").performClick()

        rule.onNodeWithText("There is a problem").assertIsDisplayed()
        assertEquals(2, rule.onAllNodesWithText("Enter what you did in your break").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("$field. Error: Enter what you did in your break").assertIsDisplayed()
        assertEquals(BreakPhase.Noting, store.wellness.state.value.clock.phase)

        rule.onNodeWithContentDescription("$field. Error: Enter what you did in your break").performTextInput("Made a tea")
        rule.onNodeWithText("Back to work").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Back to work").fetchSemanticsNodes().isEmpty() }

        assertEquals("""{"note":"Made a tea"}""", server.sent("PUT", "/note")?.third)
        assertEquals(BreakPhase.Idle, store.wellness.state.value.clock.phase)
    }
}
