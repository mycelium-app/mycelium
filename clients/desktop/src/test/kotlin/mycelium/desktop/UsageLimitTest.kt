package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.AgentsSettings
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private val ResetsToday = kotlin.time.Clock.System.now().toString().substringBefore('T') + "T15:30:00Z"
private val ResumesToday = kotlin.time.Clock.System.now().toString().substringBefore('T') + "T15:31:00Z"

private fun failedJson(resumeAt: String?) = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Plan the rollout","status":"failed",
"acp_session_id":"s1","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null,"resume_at":${resumeAt?.let { "\"$it\"" }}}"""

private fun limitEvent(resetsAt: String?) = """[{"conversation_id":"c1","seq":1,"kind":"error","payload":{"message":"Internal error: You've hit your session limit","usage_limit":{"window":"five_hour","resets_at":${resetsAt?.let { "\"$it\"" }}}}}]"""

class UsageLimitTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val calls = mutableListOf<String>()
    private val bodies = mutableListOf<String>()

    private fun store(resumeAt: String? = null, resetsAt: String? = ResetsToday, setting: String = "ask"): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method != HttpMethod.Get) {
                calls += "${request.method.value} $path"
                bodies += String(request.body.toByteArray())

                return@MockEngine respond(if (path.endsWith("/usage-limit")) """{"on_usage_limit":"resume"}""" else "", if (path.endsWith("/usage-limit")) HttpStatusCode.OK else HttpStatusCode.NoContent, json)
            }

            val body = when {
                path.endsWith("/conversations") -> "[${failedJson(resumeAt)}]"
                path.endsWith("/conversations/c1") -> failedJson(resumeAt)
                path.endsWith("/events") -> limitEvent(resetsAt)
                path.endsWith("/usage-limit") -> """{"on_usage_limit":"$setting"}"""
                path.endsWith("/queue") || path.endsWith("/nodes") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        return store
    }

    private fun show(store: Store) {
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    @Test
    fun aUsageLimitIsExplainedInPlaceOfAnInternalError() {
        show(store())

        rule.onNodeWithText("Claude Code's 5-hour limit is reached. It resets at 15:30.").assertExists()
        rule.onNodeWithContentDescription("Error: Internal error: You've hit your session limit").assertDoesNotExist()
    }

    @Test
    fun aLimitWithoutAResetTimeOffersOnlyToRetry() {
        show(store(resetsAt = null))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Claude Code's 5-hour limit is reached. Mycelium does not know when it resets.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Retry this conversation now").assertExists()
        rule.onNodeWithText("Resume at", substring = true).assertDoesNotExist()
    }

    @Test
    fun choosingToResumeSchedulesItForAMinuteAfterTheReset() {
        show(store())

        rule.onNodeWithContentDescription("Resume this conversation at 15:31").performClick()

        rule.waitUntil(5_000) { calls.isNotEmpty() }

        assertEquals(listOf("POST /api/v1/conversations/c1/resume"), calls.toList())
    }

    @Test
    fun retryingNowRetriesWithoutWaiting() {
        show(store())

        rule.onNodeWithContentDescription("Retry this conversation now").performClick()

        rule.waitUntil(5_000) { calls.isNotEmpty() }

        assertEquals(listOf("POST /api/v1/conversations/c1/retry"), calls.toList())
    }

    @Test
    fun aPendingResumeIsShownInTheThreadAndCanBeCancelledThere() {
        show(store(resumeAt = ResumesToday))

        rule.onNodeWithText("Resumes at 15:31.").assertExists()
        rule.onNodeWithContentDescription("Cancel the automatic resume").performClick()

        rule.waitUntil(5_000) { calls.isNotEmpty() }

        assertEquals(listOf("DELETE /api/v1/conversations/c1/resume"), calls.toList())
    }

    @Test
    fun aPendingResumeIsMarkedOnItsSidebarRowAndCanBeCancelledThere() {
        show(store(resumeAt = ResumesToday))

        rule.onNodeWithText("Waiting for reset").assertExists()
        rule.onNodeWithContentDescription("Cancel the automatic resume of Plan the rollout, due at 15:31").performClick()

        rule.waitUntil(5_000) { calls.isNotEmpty() }

        assertEquals(listOf("DELETE /api/v1/conversations/c1/resume"), calls.toList())
    }

    @Test
    fun theSettingDefaultsToAskAndCanBeChangedToResumeAutomatically() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AgentsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Wait and resume automatically").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("When a usage limit is reached").assertExists()
        rule.onNodeWithText("Wait and resume automatically").performClick()

        rule.waitUntil(5_000) { calls.isNotEmpty() }

        assertEquals(listOf("PUT /api/v1/usage-limit"), calls.toList())
        assertEquals("""{"on_usage_limit":"resume"}""", bodies.single())
    }
}
