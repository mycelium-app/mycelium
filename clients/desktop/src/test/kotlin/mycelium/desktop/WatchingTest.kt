package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.api.StreamMessage
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test

private fun conversationJson(id: String, title: String, status: String) = """{"id":"$id","owner":"u","node_id":"n","agent":"claude","cwd":"/work/mycelium","title":"$title","status":"$status",
"acp_session_id":"s1","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private val Watching = conversationJson("c1", "Roadmap and releases", "done")

private val Conversations = listOf(Watching, conversationJson("c2", "harbour Renovate sweep", "done"), conversationJson("c3", "atlas Release checklist", "input"))

private val AllWatches = listOf(
    """{"id":"w1","kind":"timer","note":"Check the release run finished","due_at":"2026-10-23T09:00:00Z"}""",
    """{"id":"w2","kind":"conversation","conversation_id":"c2","title":"harbour Renovate sweep"}""",
    """{"id":"w3","kind":"release","repository":"mycelium-app/mycelium","reference":"v0.28.0"}""",
    """{"id":"w4","kind":"conversation","conversation_id":"c3","title":"atlas Release checklist"}""",
)

class WatchingTest {
    @get:Rule
    val scopes = TestScopes()

    @get:Rule
    val compose = createComposeRule()

    private val requests = CopyOnWriteArrayList<String>()

    private fun show(watches: List<String>): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val remaining = CopyOnWriteArrayList(watches)
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += "${request.method.value} $path"

            when {
                path.endsWith("/conversations") -> respond(Conversations.joinToString(",", "[", "]"), HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(Watching, HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1/watches") -> respond(remaining.joinToString(",", "[", "]"), HttpStatusCode.OK, json)
                path.startsWith("/api/v1/watches/") && request.method == HttpMethod.Delete -> {
                    remaining.removeIf { it.contains("\"id\":\"${path.substringAfterLast('/')}\"") }
                    respond("", HttpStatusCode.NoContent)
                }
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
        compose.waitUntil(5_000) { requests.any { it.startsWith("GET ") && it.endsWith("/c1/watches") } }
        compose.waitForIdle()

        return store
    }

    @Test
    fun theHeaderCountsWhatTheConversationWatches() {
        show(AllWatches)

        compose.onNodeWithContentDescription("Watching 4, show the list").assertExists()
        compose.onAllNodesWithText("harbour Renovate sweep").assertCountEquals(0)
    }

    @Test
    fun theListShowsEachWatchWithItsKindAndState() {
        show(AllWatches)

        compose.onNodeWithContentDescription("Watching 4, show the list").performClick()

        compose.onNodeWithText("Check the release run finished").assertExists()
        compose.onNodeWithText("Wake-up", substring = true).assertExists()
        compose.onNodeWithContentDescription("Open harbour Renovate sweep, finished").assertExists()
        compose.onNodeWithContentDescription("Open atlas Release checklist, waiting on you").assertExists()
        compose.onNodeWithText("The v0.28.0 release of mycelium-app/mycelium").assertExists()
        compose.onNodeWithText("Release · Running").assertExists()
        compose.captureScreen("watching-list")
    }

    @Test
    fun stopRemovesThatWatchFromTheList() {
        val store = show(AllWatches)

        compose.onNodeWithContentDescription("Watching 4, show the list").performClick()
        compose.onNodeWithContentDescription("Stop watching the v0.28.0 release of mycelium-app/mycelium").performClick()
        compose.waitUntil(5_000) { requests.contains("DELETE /api/v1/watches/w3") }
        store.receive(StreamMessage.WatchesChanged("c1"))

        compose.waitUntil(5_000) { compose.onAllNodesWithText("The v0.28.0 release of mycelium-app/mycelium").fetchSemanticsNodes().isEmpty() }
        compose.onNodeWithText("Watching 3").assertExists()
    }

    @Test
    fun nothingIsShownWhenNothingIsWatched() {
        show(emptyList())

        compose.onAllNodesWithText("Watching", substring = true).assertCountEquals(0)
    }
}
