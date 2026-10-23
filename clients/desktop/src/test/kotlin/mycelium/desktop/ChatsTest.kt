package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
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
import kotlin.time.Clock
import kotlin.time.Duration.Companion.days

private object ChatPlatform : Platform {
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

private fun conversation(id: String, title: String, agent: String, updatedAt: String) =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"$agent","cwd":"/work/app","title":"$title","status":"done","created_at":"$updatedAt","updated_at":"$updatedAt"}"""

private const val Provider = """{"id":"p1","kind":"openai_compatible","name":"Ollama","base_url":"http://box:11434/v1","models":["llama3.2","qwen3"],"default_model":"llama3.2","created_at":"2026-10-22T10:00:00Z"}"""

class ChatsTest {
    @get:Rule
    val rule = createComposeRule()

    private val requests = mutableListOf<HttpRequestData>()
    private val bodies = mutableListOf<String>()
    private var providers = "[$Provider]"
    private val now = Clock.System.now()
    private val conversations = listOf(
        conversation("w1", "Fix the build", "claude", now.toString()),
        conversation("c1", "Trip to Lisbon", "chat", now.toString()),
        conversation("c2", "Compare restore strategies", "chat", (now - 3.days).toString()),
        conversation("c3", "Old recipe question", "chat", (now - 30.days).toString()),
    ).joinToString(",", "[", "]")

    private fun shell(route: Route = Route.Empty) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += request
            bodies += String(request.body.toByteArray())
            val path = request.url.encodedPath

            val body = when {
                request.method == HttpMethod.Post && path.endsWith("/chats") -> conversation("c9", "Plan a trip", "chat", now.toString())
                path.endsWith("/model-providers") -> providers
                path.endsWith("/conversations") && request.url.parameters["archived"] != "true" -> conversations
                path.endsWith("/conversations/c1") -> conversation("c1", "Trip to Lisbon", "chat", now.toString())
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, ChatPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }
    }

    @Test
    fun theSidebarStartsOnWorkspacesAndSwitchesToChats() {
        shell()

        rule.onNodeWithText("New conversation").assertIsDisplayed()
        rule.onNodeWithContentDescription("Fix the build", substring = true).assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("New chat").fetchSemanticsNodes().size)

        rule.onNodeWithText("Chats").performClick()

        rule.onNodeWithText("New chat").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("New conversation").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Fix the build", substring = true).fetchSemanticsNodes().size)

        rule.onNodeWithText("Workspaces").performClick()

        rule.onNodeWithText("New conversation").assertIsDisplayed()
    }

    @Test
    fun theSwitchIsAPairOfTabsWithTheCurrentOneSelected() {
        shell()

        rule.onNodeWithText("Workspaces").assertIsDisplayed()
        rule.onNodeWithText("Chats").performClick()
        rule.onNodeWithText("Chats").assertIsDisplayed()
    }

    @Test
    fun chatsAreGroupedUnderTodayThisWeekAndOlder() {
        shell()
        rule.onNodeWithText("Chats").performClick()

        rule.onNodeWithText("Today").assertIsDisplayed()
        rule.onNodeWithText("This week").assertIsDisplayed()
        rule.onNodeWithText("Older").assertIsDisplayed()
        rule.onNodeWithContentDescription("Trip to Lisbon", substring = true).assertIsDisplayed()
        rule.onNodeWithContentDescription("Compare restore strategies", substring = true).assertIsDisplayed()
        rule.onNodeWithContentDescription("Old recipe question", substring = true).assertIsDisplayed()
    }

    @Test
    fun openingAChatSwitchesTheSidebarToChats() {
        shell(Route.Chat("c1"))

        rule.onNodeWithText("New chat").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("New conversation").fetchSemanticsNodes().size)
    }

    @Test
    fun aChatHasNoTerminalGitOrNodeControls() {
        shell(Route.Chat("c1"))

        assertEquals(0, rule.onAllNodesWithContentDescription("Open terminal").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Open git").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Running on", substring = true).fetchSemanticsNodes().size)
    }

    @Test
    fun aWorkspaceConversationKeepsItsTerminalAndGitControls() {
        shell(Route.Chat("w1"))

        rule.onNodeWithContentDescription("Open terminal").assertIsDisplayed()
        rule.onNodeWithContentDescription("Open git").assertIsDisplayed()
    }

    @Test
    fun startingAChatSendsTheProviderModelAndFirstMessage() {
        shell(Route.NewChat)

        rule.waitUntil { rule.onAllNodesWithText("Start chat").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Message").performTextInput("Plan a trip to Lisbon")
        rule.onNodeWithText("Start chat").performClick()

        rule.waitUntil { requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/chats") } }
        assertEquals("""{"provider_id":"p1","model":"llama3.2","prompt":"Plan a trip to Lisbon"}""", bodies.last { it.contains("provider_id") })
    }

    @Test
    fun startingAChatWithoutAMessageSaysWhatToEnter() {
        shell(Route.NewChat)

        rule.waitUntil { rule.onAllNodesWithText("Start chat").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Start chat").performClick()

        rule.onNodeWithText("Enter a message to start the chat.").assertIsDisplayed()
        assertEquals(0, requests.count { it.method == HttpMethod.Post })
    }

    @Test
    fun withoutAProviderTheNewChatScreenPointsToSettings() {
        providers = "[]"
        shell(Route.NewChat)

        rule.waitUntil { rule.onAllNodesWithText("Open Settings").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("No chat model is set up. Add a provider in Settings, under Chat models, then start a chat.").assertIsDisplayed()

        rule.onNodeWithText("Open Settings").performClick()

        rule.onNodeWithText("Appearance").assertIsDisplayed()
    }
}
