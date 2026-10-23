package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.withKeyDown
import androidx.compose.ui.input.key.Key
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
import mycelium.state.PickedFile
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
    var picked = emptyList<PickedFile>()
    override fun pickFiles(onPicked: (List<PickedFile>) -> Unit) = onPicked(picked)
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

private fun conversation(id: String, title: String, agent: String, updatedAt: String, provider: String? = if (agent == "chat") "p1" else null) =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"$agent","cwd":"/work/app","title":"$title","status":"done","model":${if (provider == null) "null" else "\"llama3.2\""},${if (provider == null) "" else """"provider_id":"$provider","""}"created_at":"$updatedAt","updated_at":"$updatedAt"}"""

private const val Anthropic = """{"id":"p2","kind":"anthropic","name":"Anthropic","models":["claude-sonnet-5-5"],"default_model":"claude-sonnet-5-5","created_at":"2026-10-22T10:00:00Z"}"""

private const val Provider = """{"id":"p1","kind":"openai_compatible","name":"Ollama","base_url":"http://box:11434/v1","models":["llama3.2","qwen3"],"default_model":"llama3.2","created_at":"2026-10-22T10:00:00Z"}"""

class ChatsTest {
    @get:Rule
    val rule = createComposeRule()

    private val requests = mutableListOf<HttpRequestData>()
    private val bodies = mutableListOf<String>()
    private var providers = "[$Provider,$Anthropic]"
    private var chatEvents = "[]"
    private var removedProvider = false
    private val now = Clock.System.now()
    private val conversations get() = listOf(
        conversation("w1", "Fix the build", "claude", now.toString()),
        conversation("c1", "Trip to Lisbon", "chat", now.toString(), if (removedProvider) null else "p1"),
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
                request.method == HttpMethod.Post && path.endsWith("/attachments") -> """{"sha256":"abc","name":"notes.txt","media_type":"text/plain","size":5}"""
                request.method == HttpMethod.Patch -> conversation("c1", "Trip to Lisbon", "chat", now.toString(), "p2")
                path.endsWith("/prompt") -> ""
                path.endsWith("/conversations/c1/events") -> chatEvents
                path.endsWith("/model-providers") -> providers
                path.endsWith("/conversations") && request.url.parameters["archived"] != "true" -> conversations
                path.endsWith("/conversations/c1") -> conversation("c1", "Trip to Lisbon", "chat", now.toString(), if (removedProvider) null else "p1")
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

    private fun newChatScreen() {
        shell(Route.NewChat)

        rule.waitUntil { rule.onAllNodesWithContentDescription("Message").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theNewChatScreenIsAHeadingOverOneComposerWithAttachAndTheModelInsideIt() {
        newChatScreen()

        rule.onNodeWithText("What's on your mind?").assertIsDisplayed()
        rule.onNodeWithContentDescription("Attach files").assertIsDisplayed()
        rule.onNodeWithContentDescription("Model: Ollama · llama3.2").assertIsDisplayed()
        rule.onNodeWithContentDescription("Send").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Start chat").fetchSemanticsNodes().size)
    }

    @Test
    fun enterStartsTheChatWithTheProviderModelAndMessage() {
        newChatScreen()

        rule.onNodeWithContentDescription("Message").performTextInput("Plan a trip to Lisbon")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil { requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/chats") } }
        assertEquals("""{"provider_id":"p1","model":"llama3.2","prompt":"Plan a trip to Lisbon"}""", bodies.last { it.contains("provider_id") })
    }

    @Test
    fun shiftEnterAddsALineInsteadOfSending() {
        newChatScreen()

        rule.onNodeWithContentDescription("Message").performTextInput("First line")
        rule.onNodeWithContentDescription("Message").performKeyInput { withKeyDown(Key.ShiftLeft) { pressKey(Key.Enter) } }

        assertEquals(0, requests.count { it.method == HttpMethod.Post })
    }

    @Test
    fun theSendButtonWithoutAMessageSaysWhatToEnter() {
        newChatScreen()

        rule.onNodeWithContentDescription("Send").performClick()

        rule.onNodeWithText("Enter a message to start the chat.").assertIsDisplayed()
        assertEquals(0, requests.count { it.method == HttpMethod.Post })
    }

    @Test
    fun theModelMenuGroupsModelsByProviderAndTheChoiceIsWhatTheChatStartsWith() {
        newChatScreen()

        rule.onNodeWithContentDescription("Model: Ollama · llama3.2").performClick()

        rule.onNodeWithText("Ollama").assertIsDisplayed()
        rule.onNodeWithText("Anthropic").assertIsDisplayed()

        rule.onNodeWithText("claude-sonnet-5-5").performClick()
        rule.onNodeWithContentDescription("Model: Anthropic · claude-sonnet-5-5").assertIsDisplayed()
        rule.onNodeWithContentDescription("Message").performTextInput("Hello")
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil { requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/chats") } }
        assertEquals("""{"provider_id":"p2","model":"claude-sonnet-5-5","prompt":"Hello"}""", bodies.last { it.contains("provider_id") })
    }

    @Test
    fun anAttachedFileIsUploadedToTheNewChatAndSentWithTheMessage() {
        ChatPlatform.picked = listOf(PickedFile("notes.txt", "text/plain", "hello".encodeToByteArray()))
        newChatScreen()

        rule.onNodeWithContentDescription("Attach files").performClick()
        rule.onNodeWithContentDescription("Attached: notes.txt, 5 B").assertIsDisplayed()
        rule.onNodeWithContentDescription("Message").performTextInput("Summarise this")
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil { requests.any { it.url.encodedPath.endsWith("/prompt") } }
        ChatPlatform.picked = emptyList()
        assertEquals("""{"provider_id":"p1","model":"llama3.2"}""", bodies.last { it.contains("provider_id") })
        assertEquals("""{"text":"Summarise this","attachments":[{"sha256":"abc","name":"notes.txt","media_type":"text/plain","size":5}]}""", bodies.last { it.contains("Summarise") })
    }

    @Test
    fun anOpenChatShowsItsProviderAndModelAndSwitchingPatchesTheConversation() {
        shell(Route.Chat("c1"))

        rule.waitUntil { rule.onAllNodesWithContentDescription("Model: Ollama · llama3.2").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Model: Ollama · llama3.2").performClick()
        rule.onNodeWithText("claude-sonnet-5-5").performClick()

        rule.waitUntil { requests.any { it.method == HttpMethod.Patch } }
        assertEquals("""{"provider_id":"p2","model":"claude-sonnet-5-5"}""", bodies.last { it.contains("provider_id") })
    }

    @Test
    fun aChatWhoseProviderWasRemovedAsksForAModelInTheComposer() {
        removedProvider = true
        shell(Route.Chat("c1"))

        rule.waitUntil { rule.onAllNodesWithContentDescription("Model: Choose a model").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Model: Choose a model").performClick()
        rule.onNodeWithText("qwen3").performClick()

        rule.waitUntil { requests.any { it.method == HttpMethod.Patch } }
        assertEquals("""{"provider_id":"p1","model":"qwen3"}""", bodies.last { it.contains("provider_id") })
    }

    @Test
    fun theThreadMarksWhereTheChatSwitchedModel() {
        chatEvents = """[{"conversation_id":"c1","seq":1,"kind":"model_changed","payload":{"provider_id":"p2","provider_name":"Anthropic","model":"claude-sonnet-5-5"},"created_at":"2026-10-22T10:00:00Z"}]"""
        shell(Route.Chat("c1"))

        rule.waitUntil { rule.onAllNodesWithText("Switched to Anthropic · claude-sonnet-5-5").fetchSemanticsNodes().isNotEmpty() }
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
