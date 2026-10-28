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
import java.util.concurrent.CopyOnWriteArrayList
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

private fun goalJson(state: String, reason: String? = null, extra: String = "") =
    """{"conversation_id":"c1","text":"Every test passes on main","token_budget":5000000,"tokens_used":2100000,"turns":7,"state":"$state"${reason?.let { ""","reason":"$it"""" } ?: ""}$extra}"""

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

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Pair<String, String>>()

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
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, HandsOffPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    private fun waitForText(text: String) {
        rule.waitUntil(5_000) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun anActiveGoalShowsItsTextAndPause() {
        show(goalJson("active"))
        waitForText("Goal: Every test passes on main")

        rule.onNodeWithText("Goal: Every test passes on main").assertIsDisplayed()
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
        show(goalJson("paused", "Paused because you stopped the turn. Resume to let the agent carry on."), afterChange = goalJson("active"))
        waitForText("Resume")

        rule.onNodeWithText("Paused because you stopped the turn. Resume to let the agent carry on.").assertIsDisplayed()
        rule.onNodeWithText("Resume").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PATCH /api/v1/conversations/c1/goal" } }
        assertEquals("""{"paused":false}""", requests.last { it.first == "PATCH /api/v1/conversations/c1/goal" }.second)
    }

    @Test
    fun aGoalThatEndedShowsItsOutcomeAndOffersNoPause() {
        show(goalJson("met", "All 214 tests pass."))
        rule.waitUntil(5_000) { rule.onAllNodesWithText("All 214 tests pass.", substring = true).fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("All 214 tests pass.", substring = true).assertIsDisplayed()
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
    fun theGoalButtonSetsAGoalWithOnlyItsText() {
        show("null", afterChange = goalJson("active"))
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Set a goal").performClick()
        rule.onNodeWithContentDescription("Goal").performTextInput("Every test passes on main")
        rule.onNodeWithText("Set goal").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/conversations/c1/goal" } }
        assertEquals("""{"text":"Every test passes on main"}""", requests.last { it.first == "PUT /api/v1/conversations/c1/goal" }.second)
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

    @Test
    fun aGoalWaitingToNudgeOffersNudgeNowAndPause() {
        show(goalJson("active", extra = ""","wait_reason":"repeated_outcome","repeats":2,"next_nudge_at":"2026-10-23T14:02:00Z""""), afterChange = goalJson("active"))
        waitForText("Nudge now")

        rule.onNodeWithText("Pause").assertIsDisplayed()
        rule.onNodeWithText("Nudge now").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/conversations/c1/goal/nudge" } }
    }

    @Test
    fun aGoalHeldForAQuestionOffersNoNudgeNow() {
        show(goalJson("active", extra = ""","wait_reason":"question_waiting","held_since":"2026-10-23T14:02:00Z""""))
        waitForText("Pause")

        assertEquals(0, rule.onAllNodesWithText("Nudge now").fetchSemanticsNodes().size)
    }

    @Test
    fun aBlockedGoalContinuesWithANoteForTheAgent() {
        show(goalJson("stuck", "The CI secret is missing.", extra = ""","reported_at":"2026-10-23T14:02:00Z""""), afterChange = goalJson("active"))
        waitForText("Continue")

        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Continue the goal").assertIsDisplayed()
        rule.onNodeWithContentDescription("Note for Claude Code (optional)").performTextInput("The secret is in place.")
        rule.onAllNodesWithText("Continue")[1].performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/conversations/c1/goal/continue" } }
        assertEquals("""{"note":"The secret is in place."}""", requests.last { it.first == "POST /api/v1/conversations/c1/goal/continue" }.second)
    }

    @Test
    fun aGoalStoppedForRepeatingItselfOffersResume() {
        show(goalJson("stalled", "Stopped after 3 turns ended the same way: Two tests still fail"), afterChange = goalJson("active"))
        waitForText("Resume")

        rule.onNodeWithText("Stopped after 3 turns ended the same way: Two tests still fail. Resume to let Claude Code try again, or send it a message first.").assertIsDisplayed()
    }

    @Test
    fun aRepeatLimitThatIsNotAWholeNumberSaysHowToFixItAndSendsNothing() {
        show("null")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Set a goal").performClick()
        rule.onNodeWithContentDescription("Goal").performTextInput("Every test passes on main")
        rule.onNodeWithContentDescription("Stop after this many turns end the same way").performTextInput("often")
        rule.onNodeWithText("Set goal").performClick()

        rule.onNodeWithText("Enter how many turns, as a whole number like 3, or leave it empty.").assertIsDisplayed()
        assertEquals(false, requests.any { it.first.startsWith("PUT") })
    }

    @Test
    fun theRepeatLimitIsSentWithTheGoal() {
        show("null", afterChange = goalJson("active"))
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Set a goal").performClick()
        rule.onNodeWithContentDescription("Goal").performTextInput("Every test passes on main")
        rule.onNodeWithContentDescription("Stop after this many turns end the same way").performTextInput("4")
        rule.onNodeWithText("Set goal").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/conversations/c1/goal" } }
        assertEquals("""{"text":"Every test passes on main","repeat_limit":4}""", requests.last { it.first == "PUT /api/v1/conversations/c1/goal" }.second)
    }

    @Test
    fun theGoalBarShowsTheLimitsThatAreSetAndSaysWhenAWindowIsUnknown() {
        val started = kotlin.time.Clock.System.now() - kotlin.time.Duration.parse("40m 30s")
        show(goalJson("active", extra = ""","time_limit_seconds":7200,"started_at":"$started","five_hour_limit":80,"five_hour_used":64.2,"weekly_limit":90"""))
        waitForText("Goal: Every test passes on main")

        rule.onNodeWithText("1h 19m left", substring = true).assertIsDisplayed()
        rule.onNodeWithText("weekly usage unknown", substring = true).assertIsDisplayed()
    }

    @Test
    fun everyLimitTheDialogIsGivenIsSentWithTheGoal() {
        show("null", afterChange = goalJson("active"))
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Set a goal").performClick()
        rule.onNodeWithContentDescription("Goal").performTextInput("Every test passes on main")
        rule.onNodeWithContentDescription("Token budget").performTextInput("5M")
        rule.onNodeWithContentDescription("Time limit").performTextInput("1h 30m")
        rule.onNodeWithContentDescription("Stop when the account's 5-hour window reaches").performTextInput("80%")
        rule.onNodeWithContentDescription("Stop when the account's weekly window reaches").performTextInput("90")
        rule.onNodeWithText("Set goal").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/conversations/c1/goal" } }
        assertEquals(
            """{"text":"Every test passes on main","token_budget":5000000,"time_limit_seconds":5400,"five_hour_limit":80,"weekly_limit":90}""",
            requests.last { it.first == "PUT /api/v1/conversations/c1/goal" }.second,
        )
    }

    @Test
    fun aLimitThatIsNotReadableSaysHowToFixItAndSendsNothing() {
        show("null")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Set a goal").performClick()
        rule.onNodeWithContentDescription("Goal").performTextInput("Every test passes on main")
        rule.onNodeWithContentDescription("Time limit").performTextInput("soon")
        rule.onNodeWithContentDescription("Stop when the account's 5-hour window reaches").performTextInput("150%")
        rule.onNodeWithText("Set goal").performClick()

        rule.onNodeWithText("Enter a time limit like 2h, 45m or 1h 30m, or leave it empty.").assertIsDisplayed()
        rule.onNodeWithText("Enter a 5-hour window limit between 1 and 100 percent, like 80%, or leave it empty.").assertIsDisplayed()
        assertEquals(false, requests.any { it.first.startsWith("PUT") })
    }

    @Test
    fun aGoalThatReachedALimitSaysWhyAndOffersNoPause() {
        show(goalJson("limit_reached", "5-hour window at 82%, over your limit of 80%."))
        waitForText("5-hour window at 82%, over your limit of 80%.")

        assertEquals(0, rule.onAllNodesWithText("Pause").fetchSemanticsNodes().size)
    }
}
