package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Chat = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/backups","title":"Backups","status":"working","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private fun goalJson(state: String, reason: String? = null) =
    """{"conversation_id":"c1","text":"Every test passes on main","token_budget":5000000,"tokens_used":2100000,"turns":7,"state":"$state"${reason?.let { ""","reason":"$it"""" } ?: ""}}"""

internal object HandsOffPlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

class GoalTest {
    @get:Rule
    val rule = createComposeRule()

    private val requests = mutableListOf<Pair<String, String>>()

    private fun show(held: String, afterChange: String = held) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += "${request.method.value} $path" to String(request.body.toByteArray())
            val reply = when {
                path.endsWith("/goal") && request.method == HttpMethod.Get -> held
                path.endsWith("/goal") && request.method == HttpMethod.Delete -> ""
                path.endsWith("/goal") -> afterChange
                path.endsWith("/walkthrough") -> "null"
                path.endsWith("/nodes") -> "[]"
                path.endsWith("/conversations") -> "[$Chat]"
                path.endsWith("/conversations/c1") -> Chat
                path.endsWith("/events") || path.endsWith("/queue") -> "[]"
                else -> "{}"
            }

            respond(reply, if (request.method == HttpMethod.Delete) HttpStatusCode.NoContent else HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, HandsOffPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    private fun waitForText(text: String) {
        rule.waitUntil(5_000) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun anActiveGoalShowsItsTextProgressAndPause() {
        show(goalJson("active"))
        waitForText("Goal: Every test passes on main")

        rule.onNodeWithText("Goal: Every test passes on main").assertIsDisplayed()
        rule.onNodeWithText("Turn 7  ·  2.1M of 5M tokens").assertIsDisplayed()
        rule.onNodeWithText("Pause").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Resume").fetchSemanticsNodes().size)
    }

    @Test
    fun pausingAskstheControlPlaneToPauseTheGoal() {
        show(goalJson("active"), afterChange = goalJson("paused"))
        waitForText("Pause")

        rule.onNodeWithText("Pause").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PATCH /api/v1/conversations/c1/goal" } }
        assertEquals("""{"paused":true}""", requests.last { it.first == "PATCH /api/v1/conversations/c1/goal" }.second)
        waitForText("Resume")
        rule.onNodeWithText("Resume").assertIsDisplayed()
    }

    @Test
    fun aPausedGoalCanBeResumedAndSaysWhyItPaused() {
        show(goalJson("paused", "Paused because you sent a message. Resume to let the agent carry on."), afterChange = goalJson("active"))
        waitForText("Resume")

        rule.onNodeWithText("Paused because you sent a message. Resume to let the agent carry on.").assertIsDisplayed()
        rule.onNodeWithText("Resume").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PATCH /api/v1/conversations/c1/goal" } }
        assertEquals("""{"paused":false}""", requests.last { it.first == "PATCH /api/v1/conversations/c1/goal" }.second)
    }

    @Test
    fun aGoalThatEndedSaysSoInWordsAndOffersNoPause() {
        show(goalJson("met", "All 214 tests pass."))
        waitForText("Goal met. All 214 tests pass.")

        rule.onNodeWithText("Goal met. All 214 tests pass.").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Pause").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Resume").fetchSemanticsNodes().size)
    }

    @Test
    fun clearingTheGoalRemovesTheBar() {
        show(goalJson("active"))
        waitForText("Goal: Every test passes on main")

        rule.onNodeWithContentDescription("Clear the goal").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "DELETE /api/v1/conversations/c1/goal" } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Goal: Every test passes on main").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun theGoalButtonSetsAGoalWithABudget() {
        show("null", afterChange = goalJson("active"))
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Set a goal").performClick()
        rule.onNodeWithContentDescription("Goal").performTextInput("Every test passes on main")
        rule.onNodeWithText("Set goal").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/conversations/c1/goal" } }
        assertEquals("""{"text":"Every test passes on main","token_budget":5000000}""", requests.last { it.first == "PUT /api/v1/conversations/c1/goal" }.second)
    }

    @Test
    fun settingAGoalWithoutTextSaysHowToFixItAndSendsNothing() {
        show("null")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Set a goal").performClick()
        rule.onNodeWithText("Set goal").performClick()

        rule.onNodeWithText("Enter the goal the agent should work towards.").assertIsDisplayed()
        assertEquals(false, requests.any { it.first.startsWith("PUT") })
    }
}
