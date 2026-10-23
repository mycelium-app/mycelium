package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CompletableDeferred
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

private const val IdleJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Fix the build","status":"done",
"acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

internal object QuietPlatform : Platform {
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

class DirectSendTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun aMessageToAnIdleConversationShowsInTheChatAtOnceRatherThanAsSending() {
        val never = CompletableDeferred<Unit>()
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            when {
                path.endsWith("/prompt") && request.method == HttpMethod.Post -> never.await()
                else -> Unit
            }
            val body = when {
                path.endsWith("/conversations") -> "[$IdleJson]"
                path.endsWith("/conversations/c1") -> IdleJson
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }

        store.send("c1", "Run the tests")

        rule.onNodeWithText("Run the tests").assertExists()
        assertEquals(0, rule.onAllNodesWithContentDescription("Sending").fetchSemanticsNodes().size)
    }
}
