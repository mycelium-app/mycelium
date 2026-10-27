package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
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
import mycelium.state.Store
import mycelium.ui.MoveSheet
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Nodes = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.1.10","address":"10.0.0.5","online":true,"draining":false,"sessions":["c1"],"agents":[{"agent":"claude","available":true}],"logins":[]},
{"id":"n2","name":"build-01","hostname":"build-01","os":"linux","arch":"x86_64","version":"v0.1.10","address":"10.0.0.6","online":true,"draining":false,"sessions":[],"agents":[{"agent":"claude","available":true}],"logins":[]}]"""

private const val Chat = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/backups","title":"Backups","status":"done","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val Movable = """{"movable":true,"branch":"main","changed_files":3,"project_on_target":true,"clones_project":false,"agent_session":true,"stays_ports":[5173],"stays_processes":[]}"""

private const val Unmovable = """{"movable":false,"reason":"This folder is not on a branch. Check one out on this node, then try again.","changed_files":0,"project_on_target":true,"clones_project":false,"agent_session":true,"stays_ports":[],"stays_processes":[]}"""

private fun progress(state: String, fork: Boolean = false, message: String? = null, result: String? = null) =
    """{"conversation_id":"c1","node_id":"n2","fork":$fork,"state":"$state"${message?.let { ""","message":"$it"""" } ?: ""}${result?.let { ""","result_id":"$it"""" } ?: ""}}"""

private object MovePlatform : Platform {
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

class MoveTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())
    private val opened = java.util.Collections.synchronizedList(mutableListOf<String>())

    private fun store(plan: String, answer: String): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = String(request.body.toByteArray())
            val reply = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/move-plan") -> plan
                path.endsWith("/move") && request.method == HttpMethod.Post -> answer
                path.endsWith("/move") -> "{}"
                path.endsWith("/nodes") -> Nodes
                path.endsWith("/conversations") -> "[$Chat]"
                path.endsWith("/conversations/c1") -> Chat
                path.endsWith("/events") || path.endsWith("/queue") -> "[]"
                else -> "{}"
            }

            requests += "${request.method.value} $path" to body

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        return store
    }

    private fun sheet(store: Store) {
        val conversation = Conversation("c1", "u", "n1", "claude", "/work/backups", "Backups", ConversationStatus.Done, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                MoveSheet(store, state, conversation, onDismiss = { opened += "dismissed" }, onOpenConversation = { opened += it })
            }
        }
    }

    private fun waitForPlan() {
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Goes with it", substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun waitForText(text: String) {
        rule.waitUntil(5_000) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun waitForDescription(description: String) {
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription(description).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theNodeInTheConversationHeaderOpensAMenuWithMoveAndMoveListsTheOtherNodes() {
        val store = store(Movable, progress("moving"))
        rule.setContent { MyceliumTheme(true) { Shell(store, MovePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }

        rule.onNodeWithContentDescription("Running on willow, open the node menu").performClick()
        rule.onNodeWithText("Move").performClick()

        waitForDescription("Move to build-01")
        rule.onNodeWithContentDescription("Move to build-01").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Move to willow").fetchSemanticsNodes().size)
    }

    @Test
    fun theSheetSaysWhatGoesWithTheConversationAndWhatStaysBehind() {
        sheet(store(Movable, progress("moving")))
        waitForPlan()

        rule.onNodeWithText("3 changed files", substring = true).assertIsDisplayed()
        rule.onNodeWithText("5173", substring = true).assertIsDisplayed()
    }

    @Test
    fun moveAfterThisTurnAsksTheControlPlaneToMoveToTheChosenNode() {
        sheet(store(Movable, progress("waiting")))
        waitForPlan()

        rule.onNodeWithText("Move after this turn").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/conversations/c1/move" } }
        assertEquals("""{"node_id":"n2"}""", requests.last { it.first == "POST /api/v1/conversations/c1/move" }.second)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Waiting for the turn to end", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Cancel move").assertIsDisplayed()
    }

    @Test
    fun forkInsteadAsksForAForkAndOpensTheNewConversationWhenItIsReady() {
        sheet(store(Movable, progress("done", fork = true, result = "c2")))
        waitForPlan()

        rule.onNodeWithText("Fork instead").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/conversations/c1/move" } }
        assertEquals("""{"node_id":"n2","fork":true}""", requests.last { it.first == "POST /api/v1/conversations/c1/move" }.second)
        rule.waitUntil(5_000) { opened.size >= 2 }
        assertEquals(listOf("dismissed", "c2"), opened)
    }

    @Test
    fun aFinishedMoveClosesTheSheetAndOpensNothingElse() {
        sheet(store(Movable, progress("done", result = "c1")))
        waitForPlan()

        rule.onNodeWithText("Move after this turn").performClick()
        rule.waitUntil(5_000) { opened.isNotEmpty() }

        assertEquals(listOf("dismissed"), opened)
    }

    @Test
    fun aWaitingMoveCanBeCancelled() {
        sheet(store(Movable, progress("waiting")))
        waitForPlan()

        rule.onNodeWithText("Move after this turn").performClick()
        waitForText("Cancel move")
        rule.onNodeWithText("Cancel move").performClick()
        rule.waitUntil(5_000) { requests.any { it.first == "DELETE /api/v1/conversations/c1/move" } }

        assertEquals(true, requests.any { it.first == "DELETE /api/v1/conversations/c1/move" })
        waitForText("Move after this turn")
        rule.onNodeWithText("Move after this turn").assertIsDisplayed()
    }

    @Test
    fun aFailedMoveShowsWhatFailedAndLeavesTheButtonsToTryAgain() {
        sheet(store(Movable, progress("failed", message = "The project folder on this node has uncommitted changes. Nothing has changed: the conversation is still on willow. Choose Move to try again.")))
        waitForPlan()

        rule.onNodeWithText("Move after this turn").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("uncommitted changes", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("uncommitted changes", substring = true).assertIsDisplayed()
        rule.onNodeWithContentDescription("Dismiss the error").performClick()
        waitForText("Move after this turn")
        rule.onNodeWithText("Move after this turn").assertIsDisplayed()
    }

    @Test
    fun aConversationThatCannotMoveShowsTheReasonAndSendsNothing() {
        sheet(store(Unmovable, progress("moving")))
        val reason = "not on a branch"
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription(reason, substring = true).fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Move after this turn").performClick()
        rule.waitForIdle()

        rule.onNodeWithContentDescription(reason, substring = true).assertIsDisplayed()
        assertEquals(false, requests.any { it.first == "POST /api/v1/conversations/c1/move" })
    }
}
