package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import io.ktor.client.engine.mock.respond
import mycelium.state.ThreadItem
import mycelium.state.UsageLimitNote
import mycelium.ui.LimitActions
import mycelium.ui.MyTheme
import mycelium.ui.ThreadView
import kotlin.test.Test
import kotlin.test.assertTrue

class UsageLimitRenderTest {
    private val items = listOf(
        ThreadItem.UserMessage("e1", "Ship the release notes"),
        ThreadItem.AgentMessage("e2", "Drafting them now."),
        ThreadItem.Failure("e3", "Internal error: You've hit your session limit", UsageLimitNote("five_hour", "2026-10-23T15:30:00Z")),
    )

    private fun render(name: String, resumeAt: String?) = renderAt(name, true, false) {
        Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) {
            ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}, limitActions = LimitActions(resumeAt, {}, {}, {}), agent = "claude")
        }
    }

    @Test
    fun theSidebarRowRendersItsPendingResume() {
        val json = io.ktor.http.headersOf(io.ktor.http.HttpHeaders.ContentType, "application/json")
        val conversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Plan the rollout","status":"failed","acp_session_id":"s1","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null,"resume_at":"2026-10-23T15:31:00Z"}"""
        val engine = io.ktor.client.engine.mock.MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[$conversation]"
                path.endsWith("/conversations/c1") -> conversation
                path.endsWith("/events") -> """[{"conversation_id":"c1","seq":1,"kind":"error","payload":{"message":"x","usage_limit":{"window":"five_hour","resets_at":"2026-10-23T15:30:00Z"}}}]"""
                path.endsWith("/queue") || path.endsWith("/nodes") -> "[]"
                else -> "{}"
            }
            respond(body, io.ktor.http.HttpStatusCode.OK, json)
        }
        val store = mycelium.state.Store(mycelium.api.MyceliumApi("https://mycelium.test", "token", engine), kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Unconfined))
        kotlinx.coroutines.runBlocking { store.refresh() }

        val file = renderAt("usage-limit-sidebar", true, false) {
            mycelium.ui.Shell(store, QuietPlatform, mycelium.api.Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = mycelium.ui.Route.Chat("c1"))
        }

        assertTrue(file.length() > 5_000)
    }

    @Test
    fun theLimitCardRendersAskingAndScheduled() {
        assertTrue(render("usage-limit-ask", null).length() > 5_000)
        assertTrue(render("usage-limit-scheduled", "2026-10-23T15:31:00Z").length() > 5_000)
    }
}
