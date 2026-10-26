package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.SessionOptions
import mycelium.state.Store
import mycelium.ui.Composer
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Parent = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Fix the build","status":"review","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""
private const val Side = """{"id":"c2","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"What is the default delay","status":"review","acp_session_id":"t","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T09:00:00Z","parent_id":"c1","side_kind":"btw"}"""

private object SidePlatform : Platform {
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

class SideChatsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val conversation = Conversation("c1", "u", "n", "claude", "/work", "t", ConversationStatus.Done, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")

    private fun composer(onSideChat: (String, String) -> Unit, onSend: (String) -> Unit = {}) {
        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = false, queue = emptyList(), sending = emptyList(), onSend = onSend, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = { _, _ -> }, onRemoveQueued = {},
                    options = SessionOptions(emptyList(), null, emptyList(), null, emptyList()), commands = emptyList(),
                    onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                    onSideChat = onSideChat,
                )
            }
        }
    }

    @Test
    fun btwWithAQuestionOpensASideChatInsteadOfSendingAMessage() {
        val side = mutableListOf<Pair<String, String>>()
        val sent = mutableListOf<String>()
        composer({ kind, text -> side += kind to text }, { sent += it })

        rule.onNode(hasSetTextAction()).performTextInput("/btw what is the default delay?")
        rule.onNode(hasSetTextAction()).performKeyInput { pressKey(Key.Enter) }

        assertEquals(listOf("btw" to "what is the default delay?"), side)
        assertEquals(emptyList(), sent)
    }

    @Test
    fun theCommandListOffersBtwSideAndFork() {
        composer({ _, _ -> })

        rule.onNode(hasSetTextAction()).performTextInput("/")

        rule.onNodeWithText("btw").assertIsDisplayed()
        rule.onNodeWithText("side").assertIsDisplayed()
        rule.onNodeWithText("fork").assertIsDisplayed()
    }

    @Test
    fun sideChatsSitUnderTheirConversationInTheSidebarAndCollapseWithIt() {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") && request.url.parameters["archived"] != "true" -> "[$Parent,$Side]"
                path.endsWith("/conversations/c1") -> Parent
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, SidePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Empty) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("What is the default delay,", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("What is the default delay,", substring = true).assertIsDisplayed()

        rule.onNodeWithContentDescription("Collapse side chats of Fix the build").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("What is the default delay").fetchSemanticsNodes().isEmpty() }
        assertEquals(0, rule.onAllNodesWithText("What is the default delay").fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("Expand side chats of Fix the build").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("What is the default delay,", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("What is the default delay,", substring = true).assertIsDisplayed()
    }
}
