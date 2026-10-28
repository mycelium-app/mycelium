package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val ArchivedConversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Old work","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":"2026-10-23T10:00:00Z"}"""

class RestoreConversationTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val patches = CopyOnWriteArrayList<String>()
    private var closed = 0

    @Test
    fun restoringTheOpenConversationKeepsItOpen() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Patch) {
                patches += String(request.body.toByteArray())

                return@MockEngine respond(ArchivedConversation.replace("\"archived_at\":\"2026-10-23T10:00:00Z\"", "\"archived_at\":null"), HttpStatusCode.OK, json)
            }

            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/conversations") -> "[$ArchivedConversation]"
                path.endsWith("/conversations/c1") -> ArchivedConversation
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ConversationScreen(store, state, "c1", onArchived = { closed += 1 }) } }

        rule.onAllNodesWithContentDescription("More", substring = true)[0].performClick()
        rule.onNodeWithText("Restore").performClick()
        rule.waitUntil(5_000) { patches.isNotEmpty() }
        rule.waitForIdle()

        assertEquals(listOf("""{"archived":false}"""), patches.toList())
        assertEquals(0, closed)
    }
}
