package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextClearance
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
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.SchedulesScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val ScheduleNodes = """[{"id":"n1","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"v0.1.13","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2","available":true}],"logins":[],"sessions":[]}]"""

private fun schedule(id: String, prompt: String, cron: String, eachRun: String, paused: Boolean) =
    """{"id":"$id","node_id":"n1","agent":"claude","cwd":"/work/app","project_id":"p1","prompt":"$prompt","cron":"$cron","each_run":"$eachRun","paused":$paused,
"next_run_at":"2099-01-01T06:00:00Z","last_run_at":null,"last_conversation_id":null,"created_at":"2026-10-22T10:00:00Z"}"""

private val Schedules = "[" + listOf(
    schedule("s1", "Renovate sweep", "0 6 * * 1", "new", false),
    schedule("s2", "Check backups", "30 7 * * *", "same", false),
    schedule("s3", "Summarise the week", "0 17 * * 5", "new", true),
).joinToString(",") + "]"

private const val Runs = """[
{"id":"r1","schedule_id":"s1","ran_at":"2026-10-22T06:00:00Z","outcome":"started","reason":null,"conversation_id":"c1"},
{"id":"r2","schedule_id":"s2","ran_at":"2026-10-22T07:30:00Z","outcome":"skipped","reason":"This node is offline. Start it, then try again.","conversation_id":null}]"""

private const val RunConversation = """[{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Renovate sweep 21 Oct","status":"review","acp_session_id":"s","mode":null,"model":null,
"created_at":"2026-10-22T06:00:00Z","updated_at":"2026-10-22T06:00:00Z","archived_at":null}]"""

class SchedulesTest {
    @get:Rule
    val rule = createComposeRule()

    private val requests = mutableListOf<Triple<String, String, String>>()

    private fun showSchedules() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += Triple(request.method.value, path, String(request.body.toByteArray()))

            when {
                request.method == HttpMethod.Post && path.endsWith("/schedules/s1/run") -> respond("""{"id":"r3","schedule_id":"s1","ran_at":"2026-10-22T08:00:00Z","outcome":"started","reason":null,"conversation_id":"c1"}""", HttpStatusCode.Created, json)
                request.method == HttpMethod.Post && path.endsWith("/schedules") -> respond(schedule("s4", "Go", "0 6 * * 1", "new", false), HttpStatusCode.Created, json)
                request.method == HttpMethod.Patch -> respond(schedule("s1", "Renovate sweep", "0 6 * * 1", "new", true), HttpStatusCode.OK, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/schedule-runs") -> respond(Runs, HttpStatusCode.OK, json)
                path.endsWith("/schedules") -> respond(Schedules, HttpStatusCode.OK, json)
                path.endsWith("/projects") -> respond("""[{"id":"p1","node_id":"n1","name":"midden","path":"/work/app","created_at":"2026-10-22T10:00:00Z"}]""", HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(ScheduleNodes, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond(RunConversation, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); SchedulesScreen(store, state) } }
        rule.waitUntil { rule.onAllNodesWithText("Renovate sweep").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun sent(method: String, suffix: String) = requests.firstOrNull { it.first == method && it.second.endsWith(suffix) }

    @Test
    fun eachScheduleShowsItsPromptProjectWhenAndWhatEachRunDoes() {
        showSchedules()

        rule.onAllNodesWithText("Check backups").assertCountEquals(2)
        rule.onAllNodesWithText("midden").fetchSemanticsNodes().let { assertEquals(3, it.size) }
        rule.onNodeWithText("Mon 06:00").assertExists()
        rule.onNodeWithText("Daily 07:30").assertExists()
        rule.onNodeWithText("Fri 17:00").assertExists()
        rule.onNodeWithText("Same conversation").assertExists()
        rule.onNodeWithText("Paused").assertExists()
        rule.onNodeWithContentDescription("Resume: Summarise the week").assertExists()
    }

    @Test
    fun lastRunsShowTheConversationItStartedAndWhyARunWasSkipped() {
        showSchedules()

        rule.onNodeWithText("Renovate sweep 21 Oct").assertExists()
        rule.onNodeWithText("Ready to review").assertExists()
        rule.onNodeWithText("Skipped: This node is offline. Start it, then try again.").assertExists()
    }

    @Test
    fun runNowStartsTheScheduleAndReloadsTheRuns() {
        showSchedules()

        rule.onNodeWithContentDescription("Run now: Renovate sweep").performClick()

        rule.waitUntil { sent("POST", "/schedules/s1/run") != null }
        rule.waitUntil { requests.count { it.second.endsWith("/schedule-runs") } >= 2 }
    }

    @Test
    fun pausingASchedulePatchesItAsPaused() {
        showSchedules()

        rule.onNodeWithContentDescription("Pause: Renovate sweep").performClick()

        rule.waitUntil { sent("PATCH", "/schedules/s1") != null }
        assertEquals("""{"paused":true}""", sent("PATCH", "/schedules/s1")?.third)
    }

    @Test
    fun resumingAPausedScheduleSendsPausedFalse() {
        showSchedules()

        rule.onNodeWithContentDescription("Resume: Summarise the week").performClick()

        rule.waitUntil { sent("PATCH", "/schedules/s3") != null }
        assertEquals("""{"paused":false}""", sent("PATCH", "/schedules/s3")?.third)
    }

    @Test
    fun deletingAScheduleRemovesItFromThePage() {
        showSchedules()

        rule.onNodeWithContentDescription("Delete: Check backups").performClick()

        rule.waitUntil { rule.onAllNodesWithText("Check backups").fetchSemanticsNodes().isEmpty() }
        assertTrue(sent("DELETE", "/schedules/s2") != null)
    }

    @Test
    fun aNewWeeklyScheduleIsSentAsACronExpressionWithItsFolder() {
        showSchedules()

        rule.onNodeWithText("New schedule").performClick()
        rule.onNodeWithContentDescription("Prompt").performTextInput("Go")
        rule.onNodeWithContentDescription("Folder").performTextInput("/work/app")
        rule.onNodeWithText("Add schedule").performClick()

        rule.waitUntil { sent("POST", "/schedules") != null }
        assertEquals("""{"node_id":"n1","agent":"claude","prompt":"Go","cron":"0 6 * * 1","each_run":"new","cwd":"/work/app"}""", sent("POST", "/schedules")?.third)
    }

    @Test
    fun aDailyScheduleThatReusesTheConversationSendsItsTimeAndChoice() {
        showSchedules()

        rule.onNodeWithText("New schedule").performClick()
        rule.onNodeWithContentDescription("Prompt").performTextInput("Go")
        rule.onNodeWithText("Daily").performClick()
        rule.onNodeWithContentDescription("At (UTC)").performTextClearance()
        rule.onNodeWithContentDescription("At (UTC)").performTextInput("07:30")
        rule.onAllNodesWithText("Same conversation").onLast().performClick()
        rule.onNodeWithContentDescription("Folder").performTextInput("/work/app")
        rule.onNodeWithText("Add schedule").performClick()

        rule.waitUntil { sent("POST", "/schedules") != null }
        assertEquals("""{"node_id":"n1","agent":"claude","prompt":"Go","cron":"30 7 * * *","each_run":"same","cwd":"/work/app"}""", sent("POST", "/schedules")?.third)
    }

    @Test
    fun aScheduleWithoutAPromptOrAValidTimeIsRefusedWithWhatToFix() {
        showSchedules()

        rule.onNodeWithText("New schedule").performClick()
        rule.onNodeWithText("Daily").performClick()
        rule.onNodeWithContentDescription("At (UTC)").performTextClearance()
        rule.onNodeWithContentDescription("At (UTC)").performTextInput("7.30")
        rule.onNodeWithText("Add schedule").performClick()

        rule.onNodeWithText("Enter the prompt to run on this schedule.").assertExists()
        rule.onNodeWithText("Enter the time as hours and minutes, for example 06:30.").assertExists()
        assertTrue(sent("POST", "/schedules") == null)
    }
}
