package mycelium.desktop

import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.width
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.TerminalPanel
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertTrue

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work/midden","title":"Bump","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

@OptIn(InternalComposeUiApi::class)
class TerminalPanelTest {
    private val requests = CopyOnWriteArrayList<String>()
    private val bodies = CopyOnWriteArrayList<String>()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += "${request.method.value} ${request.url.encodedPath}"
            bodies += String(request.body.toByteArray())

            if (request.url.encodedPath.endsWith("/terminal")) respond("""{"terminal_id":"t1"}""", HttpStatusCode.OK, json) else respond("", HttpStatusCode.NoContent)
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
    }

    private fun waitFor(scene: ImageComposeScene?, what: String, done: () -> Boolean) {
        var frame = 0

        while (!done() && frame < 500) {
            scene?.render(frame * 16_000_000L)
            frame++
            Thread.sleep(10)
        }

        assertTrue(done(), "timed out waiting for $what")
    }

    @Test
    fun openingThePanelStartsATerminalSizedToItsAreaAndClosingItStopsIt() {
        val conversation = MyceliumJson.decodeFromString(Conversation.serializer(), ConversationJson)
        val store = store()
        val scene = ImageComposeScene(600, 700, Density(1f)) {
            MyceliumTheme(true) { TerminalPanel(store, AppState(), conversation, Modifier.width(440.dp).fillMaxHeight()) }
        }

        waitFor(scene, "the terminal to start") { requests.any { it == "POST /api/v1/conversations/c1/terminal" } }

        val size = Json.parseToJsonElement(bodies[requests.indexOf("POST /api/v1/conversations/c1/terminal")]).jsonObject
        assertTrue(size.getValue("cols").jsonPrimitive.int in 50..70, "cols were $size")
        assertTrue(size.getValue("rows").jsonPrimitive.int in 28..40, "rows were $size")

        repeat(10) { scene.render(it * 16_000_000L + 10_000_000_000L) }
        scene.close()
        waitFor(null, "the terminal to stop") { requests.contains("DELETE /api/v1/terminals/t1") }
    }
}
