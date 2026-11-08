package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test

private const val WatchingConversationJson = """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work/mycelium","title":"Roadmap and releases","status":"done",
"acp_session_id":"s1","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val WatchesJson = """[
{"id":"w1","kind":"timer","note":"Check the release run finished","due_at":"2026-10-23T09:00:00Z"},
{"id":"w2","kind":"conversation","conversation_id":"c2","title":"harbour Renovate sweep"},
{"id":"w3","kind":"release","repository":"mycelium-app/mycelium","reference":"v0.28.0"}
]"""

class WatchLinesTest {
    @get:Rule
    val scopes = TestScopes()

    @get:Rule
    val compose = createComposeRule()

    private val requests = CopyOnWriteArrayList<String>()

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += "${request.method.value} $path"

            when {
                path.endsWith("/conversations") -> respond("[$WatchingConversationJson]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(WatchingConversationJson, HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1/watches") -> respond(WatchesJson, HttpStatusCode.OK, json)
                path.startsWith("/api/v1/watches/") -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/terminals") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.open("c1") }
        compose.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) {
                    ConversationScreen(store, state, "c1", onArchived = {}, panels = false)
                }
            }
        }
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Check the release run finished", substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aPendingWakeUpAndEachWatchShowAboveTheThread() {
        show()

        compose.onNodeWithText("Wakes", substring = true).assertExists()
        compose.onNodeWithContentDescription("Open harbour Renovate sweep").assertExists()
        compose.onNodeWithText("Watching the v0.28.0 release of mycelium-app/mycelium").assertExists()
        compose.captureScreen("watch-lines")
    }

    @Test
    fun stopEndsThatWatch() {
        show()

        compose.onNodeWithContentDescription("Stop watching the v0.28.0 release of mycelium-app/mycelium").performClick()

        compose.waitUntil(5_000) { requests.contains("DELETE /api/v1/watches/w3") }
    }
}
