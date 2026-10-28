package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.ModelProvider
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.NodeSummary
import mycelium.state.PickerAccount
import mycelium.state.Store
import mycelium.state.routingChoices
import mycelium.ui.AccountPicker
import mycelium.ui.AccountPickerControls
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val WillowAndAtlas = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.11.0","address":"10.0.0.5","online":true,"draining":false,"sessions":["c1"],
"agents":[{"agent":"claude","available":true},{"agent":"codex","available":true}],
"logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in","usage_left":18},{"agent":"codex","account":"codex@example.com","method":"token","state":"signed_in"}]},
{"id":"n2","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"v0.11.0","address":"10.0.0.6","online":true,"draining":false,"sessions":[],
"agents":[{"agent":"claude","available":true}],
"logins":[{"agent":"claude","account":"work@example.org","method":"token","state":"signed_in","usage_left":64}]}]"""

private const val Started = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Backups","status":"done","acp_session_id":"s","account":"sam@example.com","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val FolderMissing = """{"error":"folder_missing","message":"The folder /work/app is not on atlas."}"""

private object PickerPlatform : Platform {
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

class AccountPickerTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())

    private val nodes = MyceliumJson.decodeFromString<List<NodeSummary>>(WillowAndAtlas)

    private val providers = listOf(ModelProvider("p1", "ollama", "Ollama", models = listOf("llama3", "qwen3"), favourites = listOf("qwen3"), createdAt = "2026-10-22T10:00:00Z"))

    private fun row(node: NodeSummary, agent: String, account: String, usage: String, here: Boolean) = PickerAccount("on:${node.id}:$agent:$account", agent, account, node, here, usage)

    private fun showPicker(onPick: (String) -> Unit = {}) {
        val willow = nodes.first { it.name == "willow" }
        val atlas = nodes.first { it.name == "atlas" }
        val accounts = listOf(
            row(atlas, "claude", "work@example.org", "64% left", false),
            row(willow, "claude", "sam@example.com", "18% left", true),
            row(willow, "codex", "codex@example.com", "Usage unknown", true),
        )

        rule.setContent {
            MyceliumTheme(true) {
                AccountPicker(
                    AccountPickerControls(
                        title = "Continue on",
                        onPick = { picked, done -> onPick(picked); done(null) },
                        label = "Claude Code · sam@example.com",
                        accounts = accounts,
                        routing = routingChoices(nodes.flatMap { it.logins }, emptyList(), "moves to"),
                        providers = providers,
                    ),
                )
            }
        }

        rule.onNodeWithContentDescription("Account: Claude Code · sam@example.com").performClick()
    }

    private fun shell(continueReply: Pair<HttpStatusCode, String>? = null, route: Route = Route.Chat("c1"), account: String = "Account: Claude Code · sam@example.com") {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val reply = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/continue") -> continueReply?.second ?: Started
                path.endsWith("/nodes") -> WillowAndAtlas
                path.endsWith("/conversations") -> "[$Started]"
                path.endsWith("/conversations/c1") -> Started
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/model-providers") -> "[]"
                else -> "{}"
            }

            posts += "${request.method.value} $path" to String(request.body.toByteArray())

            respond(reply, if (path.endsWith("/continue")) continueReply?.first ?: HttpStatusCode.OK else HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, PickerPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription(account).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription(account).performClick()
    }

    @Test
    fun thePickerOpensWithASearchFieldAndOneHeadingPerSection() {
        showPicker()

        rule.onNodeWithContentDescription("Search accounts, nodes and models").assertIsDisplayed()

        for (heading in listOf("Favourites", "Switch as usage runs out", "Claude Code", "Codex", "Chat models")) {
            rule.onNode(hasScrollAction()).performScrollToNode(hasText(heading))
            rule.onNodeWithText(heading).assertIsDisplayed()
        }
    }

    @Test
    fun typingANodeNameKeepsOnlyThatNodesRows() {
        showPicker()

        rule.onNodeWithContentDescription("Search accounts, nodes and models").performTextInput("atlas")

        rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Claude Code · sam@example.com on this device, 18% left").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Chat models").fetchSemanticsNodes().size)
    }

    @Test
    fun anAccountOnAnotherNodeNamesTheNodeAndOneOnThisConversationsNodeSaysThisDevice() {
        showPicker()

        rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").assertIsDisplayed()
        rule.onNodeWithContentDescription("Claude Code · sam@example.com on this device, 18% left").assertIsDisplayed()
        rule.onAllNodesWithText("This device", useUnmergedTree = true).assertCountEquals(2)
    }

    @Test
    fun theFavouritesAndChatModelsListsAreSearchableFromTheSameField() {
        showPicker()

        rule.onAllNodesWithContentDescription("Remove qwen3 from favourites").onFirst().assertExists()
        rule.onNodeWithContentDescription("Search accounts, nodes and models").performTextInput("llama")

        rule.onNodeWithText("llama3").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Favourites").fetchSemanticsNodes().size)
    }

    @Test
    fun choosingAnAccountOnAnotherNodeContinuesThereWithThatNodeId() {
        shell()

        rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").performClick()

        rule.waitUntil(5_000) { posts.toList().any { it.first == "POST /api/v1/conversations/c1/continue" } }
        assertEquals("""{"agent":"claude","account":"work@example.org","node_id":"n2"}""", posts.toList().last { it.first == "POST /api/v1/conversations/c1/continue" }.second)
    }

    @Test
    fun aRefusalBecauseTheFolderIsMissingShowsTheAlertInsideThePickerWithAMoveAction() {
        shell(HttpStatusCode.Conflict to FolderMissing)

        rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Error: The folder /work/app is not on atlas.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Move to atlas").assertIsDisplayed()
        rule.onNodeWithContentDescription("Claude Code · sam@example.com on this device, 18% left").assertExists()

        val row = rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").getBoundsInRoot()
        val alert = rule.onNodeWithContentDescription("Error: The folder /work/app is not on atlas.").getBoundsInRoot()

        assertTrue(alert.top >= row.bottom, "the alert sits under the refused row")
    }

    @Test
    fun onTouchTheSheetKeepsTheAccessibleName() {
        rule.setContent {
            CompositionLocalProvider(LocalTouch provides true) {
                MyceliumTheme(true) {
                    AccountPicker(AccountPickerControls(title = "Start on", onPick = { _, done -> done(null) }, label = "Claude Code · sam@example.com", providers = providers))
                }
            }
        }

        rule.onNodeWithContentDescription("Account: Claude Code · sam@example.com").performClick()

        rule.onNodeWithContentDescription("Start on").assertExists()
    }

    @Test
    fun thePickerHasItsAccessibleNameInAnExistingConversation() {
        shell()

        rule.onNodeWithContentDescription("Continue on").assertExists()
    }

    @Test
    fun choosingAnAccountOnANodeStartsTheNewChatThereAndChecksThatRow() {
        shell(route = Route.NewChat, account = "Account: Choose a model")

        rule.onNodeWithContentDescription("Start on").assertExists()
        rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").performClick()
        rule.onNodeWithContentDescription("Message").performTextInput("Hello")
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil(5_000) { posts.toList().any { it.first == "POST /api/v1/chats" } }
        assertEquals("""{"prompt":"Hello","agent":"claude","account":"work@example.org","node_id":"n2"}""", posts.toList().last { it.first == "POST /api/v1/chats" }.second)
    }

    @Test
    fun theChosenNewChatRowIsTheOneChecked() {
        shell(route = Route.NewChat, account = "Account: Choose a model")

        rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").performClick()
        rule.onNodeWithContentDescription("Account: Claude Code · work@example.org").performClick()

        rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").assertIsSelected()
        rule.onNodeWithContentDescription("Claude Code · sam@example.com on willow, 18% left").assertIsNotSelected()
    }
}
