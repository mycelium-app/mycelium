package mycelium.desktop

import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.performClick
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import mycelium.api.Me
import mycelium.ui.MyceliumTheme
import mycelium.ui.Shell
import org.junit.Rule

class StartBreakNowTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = HoursPlatform("all")
    private val server = HoursServer(listOf(hoursGroupJson("g-personal", "personal")), listOf(hoursConversationJson("c1", "Personal chat", "g-personal", "working")))
    private val startBreakNow = SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button) and hasText("Start a break now")

    private fun openWellness() {
        val store = server.store(scopes.create(), platform)

        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Personal chat").fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText("Wellness & Learning").onFirst().performClick()
        rule.waitUntil(5_000) { rule.onAllNodes(startBreakNow).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun startingABreakFromTheWellnessPageOpensTheBreakScreenAndPostsTheBreak() {
        openWellness()

        rule.onAllNodes(startBreakNow).onFirst().performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("End break early").fetchSemanticsNodes().isNotEmpty() }

        assertNotNull(server.sent("POST", "/wellness/breaks"))
    }

    @Test
    fun theControlIsNotShownWhileABreakIsRunning() {
        openWellness()

        rule.onAllNodes(startBreakNow).onFirst().performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("End break early").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(0, rule.onAllNodes(startBreakNow).fetchSemanticsNodes().size)
    }
}
