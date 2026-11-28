package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.Accent
import mycelium.ui.Appearance
import mycelium.ui.LocalAppearance
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.Test
import kotlin.test.assertTrue

private const val Host = "77777777-7777-7777-7777-777777777777"
private const val Captain = """{"id":"as1","name":"Release captain","colour":"violet","preferences":{},"conversation_id":"$Host","state":"idle","session_count":1,"created_at":"2026-10-01T10:00:00Z","secrets":[]}"""
private const val Routing = """{"id":"vc1","assistant_id":"as1","title":"Discord routing rules","closed":false,"unread":false,"todos":[{"content":"Pick a variant","status":"completed"},{"content":"Owner builds the rules page","status":"in_progress"},{"content":"Release","status":"pending"}],"created_at":"2026-11-28T10:00:00Z","updated_at":"2026-11-28T10:00:00Z"}"""
private const val MacMini = """{"id":"vc2","assistant_id":"as1","title":"Mac mini install","closed":true,"unread":false,"todos":[],"created_at":"2026-11-28T09:00:00Z","updated_at":"2026-11-28T09:00:00Z"}"""
private const val Question = """{"id":"m1","chat_id":"vc1","from_person":false,"text":"Three ways to lay out the rules. Which one?","sending":false,"created_at":"2026-11-28T10:01:00Z"}"""
private const val Waiting = """{"id":"m2","chat_id":"vc1","from_person":true,"text":"can the fallback be any server?","sending":true,"created_at":"2026-11-28T10:02:00Z"}"""

class VirtualChatsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val sent = CopyOnWriteArrayList<Pair<String, String>>()

    private fun show(route: Route): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method.value != "GET") {
                sent += "${request.method.value} $path" to String(request.body.toByteArray())
            }

            val body = when {
                request.method.value == "POST" && path.endsWith("/virtual-chats/vc1/messages") -> """{"id":"m3","chat_id":"vc1","from_person":true,"text":"C","sending":false,"created_at":"2026-11-28T10:03:00Z"}"""
                request.method.value == "PATCH" && path.endsWith("/virtual-chats/vc1") -> Routing
                request.method.value == "DELETE" -> ""
                path.endsWith("/assistants/as1/virtual-chats") -> "[$Routing,$MacMini]"
                path.endsWith("/virtual-chats/vc1/messages") -> "[$Question,$Waiting]"
                path.endsWith("/assistants") -> "[$Captain]"
                path.endsWith("/conversations") || path.endsWith("/events") || path.endsWith("/queue") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking {
            runCatching { store.refreshAssistants() }
            runCatching { store.refreshVirtualChats("as1") }
        }

        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalAppearance provides Appearance(true, Accent.Neutral, Accent.entries.filter { it != Accent.System }, {}), LocalPlatform provides PlatformStub) {
                    Box(Modifier.size(1440.dp, 900.dp)) {
                        Shell(store, PlatformStub, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route)
                    }
                }
            }
        }

        return store
    }

    private fun waitFor(text: String) {
        rule.waitUntil(5_000) { rule.onAllNodes(hasText(text)).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun anAssistantsVirtualChatsAreListedUnderItAndAClosedOneSaysSo() {
        show(Route.Assistants)

        waitFor("Discord routing rules")
        rule.onNodeWithContentDescription("Discord routing rules").assertIsDisplayed()
        rule.onNodeWithContentDescription("Mac mini install, closed").assertIsDisplayed()
        rule.onNodeWithContentDescription("New virtual chat for Release captain").assertIsDisplayed()
    }

    @Test
    fun aVirtualChatShowsTheAssistantsMessagesItsToDoListAndAMessageStillBeingSent() {
        show(Route.VirtualChat("as1", "vc1"))

        waitFor("Three ways to lay out the rules. Which one?")
        rule.onNodeWithText("Owner builds the rules page").assertIsDisplayed()
        rule.onNodeWithText("2 of 3").assertIsDisplayed()
        rule.onNodeWithText("Sending").assertIsDisplayed()
        rule.onNodeWithContentDescription("Attach files").assertIsDisplayed()
    }

    @Test
    fun cancellingAMessageNotYetReadPutsItBackInTheComposer() {
        show(Route.VirtualChat("as1", "vc1"))

        waitFor("Sending")
        rule.onNodeWithContentDescription("Cancel sending this message").performClick()

        rule.waitUntil(5_000) { sent.any { it.first == "DELETE /api/v1/virtual-chats/vc1/messages/m2" } }
        waitFor("can the fallback be any server?")
        rule.waitUntil(5_000) { rule.onAllNodes(hasText("Sending")).fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun aMessageWrittenInTheChatIsSentToIt() {
        show(Route.VirtualChat("as1", "vc1"))

        waitFor("Three ways to lay out the rules. Which one?")
        rule.onNodeWithContentDescription("Message").performTextInput("C")
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil(5_000) { sent.any { it.first == "POST /api/v1/virtual-chats/vc1/messages" } }
        assertTrue(sent.first { it.first == "POST /api/v1/virtual-chats/vc1/messages" }.second.contains("\"text\":\"C\""))
    }

    @Test
    fun closingAVirtualChatIsInItsMoreMenu() {
        show(Route.VirtualChat("as1", "vc1"))

        waitFor("Three ways to lay out the rules. Which one?")
        rule.onNodeWithContentDescription("More for Discord routing rules").performClick()
        rule.onNodeWithText("Close virtual chat").performClick()

        rule.waitUntil(5_000) { sent.any { it.first == "PATCH /api/v1/virtual-chats/vc1" && it.second.contains("\"closed\":true") } }
    }
}
