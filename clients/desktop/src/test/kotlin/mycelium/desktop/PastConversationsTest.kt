package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithContentDescription
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
import mycelium.ui.AgentsSettings
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val NodeId = "aaaaaaaa-0000-0000-0000-000000000001"

private const val NodesJson = """[{"id":"$NodeId","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[]}]"""

private const val ArchivedJson = """[{"id":"c1","owner":"u","node_id":"$NodeId","agent":"claude","cwd":"/home/sam/projects/okapi","title":"Plan the rollout","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":"2026-10-22T10:00:00Z"}]"""

private const val UsageJson = """[{"key":"c1","name":"","turns":3,"reported_turns":3,"input_tokens":1000,"output_tokens":500,"cost_usd":0.5,"cost_estimated":true}]"""

private object NoPlatform : Platform {
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

class PastConversationsTest {
    @get:Rule
    val rule = createComposeRule()

    private val importFinishes = CompletableDeferred<Unit>()
    private val deleted = mutableListOf<String>()

    private fun store(usage: String = UsageJson, current: String = "[]"): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Delete) {
                deleted += path

                return@MockEngine respond("", HttpStatusCode.NoContent)
            }

            val body = when {
                path.endsWith("/activity/conversations") -> usage
                path.endsWith("/imports") && request.method == HttpMethod.Post -> {
                    importFinishes.await()
                    """{"imported":3,"skipped":0}"""
                }
                path.endsWith("/imports") -> """[{"node_id":"$NodeId","agent":"claude","found":3,"imported":0}]"""
                path.endsWith("/nodes") -> NodesJson
                path.endsWith("/conversations") && request.url.parameters["archived"] == "true" -> ArchivedJson
                path.endsWith("/conversations") -> current
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        runBlocking { store.refresh() }

        return store
    }

    @Test
    fun historyNamesTheProjectEachPastConversationWasFor() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, NoPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Archive) } }

        rule.onNodeWithText("Project").assertExists()
        rule.onNodeWithText("okapi").assertExists()
    }

    @Test
    fun historyShowsTurnsTokensAndAnEstimatedCostForEachConversation() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, NoPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Archive) } }

        rule.onNodeWithText("Turns").assertExists()
        rule.onNodeWithText("Tokens").assertExists()
        rule.onNodeWithText("Cost").assertExists()
        rule.onNodeWithText("3").assertExists()
        rule.onNodeWithText("1.5k").assertExists()
        rule.onNodeWithText("~$0.50 (estimated)").assertExists()
    }

    @Test
    fun historySaysNotReportedForAConversationWithNoUsage() {
        val store = store(usage = "[]")
        rule.setContent { MyceliumTheme(true) { Shell(store, NoPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Archive) } }

        rule.onAllNodesWithText("Not reported").assertCountEquals(2)
    }

    @Test
    fun deletingFromHistoryAsksFirstNamingWhatIsLostThenRemovesTheRow() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, NoPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Archive) } }

        rule.onNodeWithContentDescription("Delete Plan the rollout").performClick()
        rule.onNodeWithText("Delete Plan the rollout?").assertExists()
        rule.onNodeWithText("Its messages and attachments are removed. This cannot be undone.").assertExists()
        assertEquals(emptyList(), deleted)

        rule.onNodeWithText("Delete conversation").performClick()

        rule.waitUntil { rule.onAllNodesWithContentDescription("Open Plan the rollout").fetchSemanticsNodes().isEmpty() }
        assertEquals(listOf("/api/v1/conversations/c1"), deleted)
    }

    @Test
    fun cancellingTheDeleteKeepsTheConversation() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, NoPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Archive) } }

        rule.onNodeWithContentDescription("Delete Plan the rollout").performClick()
        rule.onNodeWithText("Cancel").performClick()

        rule.onNodeWithContentDescription("Open Plan the rollout").assertExists()
        assertEquals(emptyList(), deleted)
    }

    @Test
    fun anArchivedConversationCanBeDeletedFromItsHeaderMenuAfterConfirming() {
        val store = store(current = ArchivedJson)
        rule.setContent { MyceliumTheme(true) { Shell(store, NoPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }

        rule.onNodeWithContentDescription("More").performClick()
        rule.onNodeWithText("Delete").performClick()
        rule.onNodeWithText("Delete Plan the rollout?").assertExists()
        rule.onNodeWithText("Delete conversation").performClick()

        rule.waitUntil { deleted.isNotEmpty() }
        assertEquals(listOf("/api/v1/conversations/c1"), deleted)
    }

    @Test
    fun anImportShowsProgressUntilItFinishes() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AgentsSettings(store) } }

        rule.onNodeWithText("Import 3").performClick()
        rule.onNodeWithContentDescription("Importing Claude Code conversations from laptop").assertExists()

        importFinishes.complete(Unit)
        rule.waitUntil { rule.onAllNodesWithContentDescription("Importing Claude Code conversations from laptop").fetchSemanticsNodes().isEmpty() }
    }
}
