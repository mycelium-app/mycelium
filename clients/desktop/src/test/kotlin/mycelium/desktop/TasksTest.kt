package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
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
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.TasksScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private class BoardPlatform(val saved: MutableMap<String, String> = mutableMapOf()) : Platform {
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
    override fun loadPreference(key: String): String? = saved[key]
    override fun savePreference(key: String, value: String) {
        saved[key] = value
    }
}

private fun conversation(id: String, title: String, status: String, cwd: String = "/other", agent: String = "claude", model: String = "null") =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"$agent","cwd":"$cwd","title":"$title","status":"$status","acp_session_id":"s","mode":null,"model":$model,
"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val BoardNodes = """[{"id":"n1","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"v0.1.13","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2","available":true},{"agent":"codex","version":"1","available":true}],"logins":[],"sessions":[]}]"""

private val BoardConversations = "[" + listOf(
    conversation("c-input", "Waiting on you", "input", "/work/app"),
    conversation("c-run", "Running job", "working", "/work/app-wt/feature-x", model = "\"opus\""),
    conversation("c-review", "Ready for a look", "review", agent = "codex"),
    conversation("c-failed", "Broke down", "failed"),
    conversation("c-done", "Shipped it", "done"),
).joinToString(",") + "]"

private const val PermissionEvent = """[{"conversation_id":"c-input","seq":1,"kind":"permission_request","payload":{"request_id":"r1","toolCall":{"title":"Run npm test"},
"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}]"""

class TasksTest {
    @get:Rule
    val rule = createComposeRule()

    private val posts = mutableListOf<Pair<String, String>>()
    private val platform = BoardPlatform()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post) {
                posts += path to String(request.body.toByteArray())
            }

            when {
                request.method == HttpMethod.Post && path.endsWith("/tasks/t1/start") -> respond(conversation("c-new", "Rebuild install.md", "working", "/work/app"), HttpStatusCode.Created, json)
                request.method == HttpMethod.Post && path.endsWith("/tasks") -> respond("""{"id":"t2","node_id":"n1","agent":"claude","cwd":"/srv","title":"Tidy","created_at":"2026-10-22T10:00:00Z"}""", HttpStatusCode.Created, json)
                path.endsWith("/conversations/c-input/events") -> respond(PermissionEvent, HttpStatusCode.OK, json)
                path.endsWith("/events") || path.endsWith("/queue") -> respond("[]", HttpStatusCode.OK, json)
                path.endsWith("/tasks") -> respond("""[{"id":"t1","node_id":"n1","agent":"claude","cwd":"/work/app","project_id":"p1","branch":null,"title":"Rebuild install.md","created_at":"2026-10-22T10:00:00Z"}]""", HttpStatusCode.OK, json)
                path.endsWith("/projects") -> respond("""[{"id":"p1","node_id":"n1","name":"app","path":"/work/app","created_at":"2026-10-22T10:00:00Z"}]""", HttpStatusCode.OK, json)
                path.endsWith("/worktrees") -> respond("""[{"id":"w1","project_id":"p1","branch":"feature/x","path":"/work/app-wt/feature-x","created_at":"2026-10-22T10:00:00Z"}]""", HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(BoardNodes, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond(BoardConversations, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        runBlocking { store.refresh() }

        return store
    }

    private fun showBoard(store: Store = store(), onOpen: (String) -> Unit = {}) {
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); TasksScreen(store, state, platform, onOpen) } }
        rule.waitUntil { rule.onAllNodesWithText("Rebuild install.md").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun conversationsSitInTheLaneOfTheirStatusAndBacklogTasksInToDo() {
        showBoard()

        for (title in listOf("Waiting on you", "Running job", "Ready for a look", "Broke down", "Rebuild install.md")) {
            rule.onNodeWithText(title).assertExists()
        }

        for (lane in listOf("Needs input", "To do", "Running", "To review")) {
            rule.onNodeWithText(lane).assertExists()
        }
    }

    @Test
    fun aCardShowsAgentModelProjectAndBranch() {
        showBoard()

        rule.onNodeWithText("Claude Code · opus · app · feature/x").assertExists()
        rule.onNodeWithText("Codex").assertExists()
    }

    @Test
    fun runningCardsShowHowLongTheyHaveBeenRunning() {
        showBoard()

        rule.onNodeWithContentDescription("Running for", substring = true).assertExists()
    }

    @Test
    fun doneStartsCollapsedAndExpandingItShowsItsCardsAndRemembersTheChoice() {
        showBoard()

        rule.onNodeWithText("Shipped it").assertDoesNotExist()
        rule.onNodeWithContentDescription("Expand Done, 1").performScrollTo().performClick()
        rule.onNodeWithText("Shipped it").assertExists()
        rule.onNodeWithContentDescription("Collapse Done").assertExists()
        assertEquals("", platform.saved.getValue("tasks-collapsed-lanes"))
    }

    @Test
    fun aLaneCollapsedEarlierStaysCollapsedWhenTheBoardIsOpenedAgain() {
        platform.saved["tasks-collapsed-lanes"] = "Running,Done"
        showBoard()

        rule.onNodeWithText("Running job").assertDoesNotExist()
        rule.onNodeWithContentDescription("Expand Running, 1").assertExists()
        rule.onNodeWithText("Ready for a look").assertExists()
    }

    @Test
    fun collapsingALaneHidesItsCardsAndSavesTheCollapsedLanes() {
        showBoard()

        rule.onNodeWithContentDescription("Collapse Running").performClick()

        rule.onNodeWithText("Running job").assertDoesNotExist()
        assertEquals(setOf("Done", "Running"), platform.saved.getValue("tasks-collapsed-lanes").split(',').toSet())
    }

    @Test
    fun theTextFilterNarrowsEveryLane() {
        showBoard()

        rule.onNodeWithContentDescription("Filter tasks").performTextInput("running")

        rule.onNodeWithText("Running job").assertExists()
        rule.onNodeWithText("Waiting on you").assertDoesNotExist()
        rule.onNodeWithText("Rebuild install.md").assertDoesNotExist()
    }

    @Test
    fun theProjectFilterKeepsOnlyCardsOfThatProject() {
        showBoard()

        rule.onNodeWithContentDescription("Project: All projects").performClick()
        rule.onNodeWithText("app").performClick()

        rule.onNodeWithText("Waiting on you").assertExists()
        rule.onNodeWithText("Rebuild install.md").assertExists()
        rule.onNodeWithText("Ready for a look").assertDoesNotExist()
    }

    @Test
    fun theAgentFilterKeepsOnlyCardsOfThatAgent() {
        showBoard()

        rule.onNodeWithContentDescription("Agent: All agents").performClick()
        rule.onAllNodesWithText("Codex").onLast().performClick()

        rule.onNodeWithText("Ready for a look").assertExists()
        rule.onNodeWithText("Running job").assertDoesNotExist()
    }

    @Test
    fun theNodeFilterOffersTheNodesAndKeepsTheirCards() {
        showBoard()

        rule.onNodeWithContentDescription("Node: All nodes").performClick()
        rule.onNodeWithText("laptop").performClick()

        rule.onNodeWithText("Running job").assertExists()
    }

    @Test
    fun stoppingARunningAgentFromItsCardCancelsTheConversation() {
        showBoard()

        rule.onNodeWithContentDescription("Stop Running job").performClick()

        rule.waitUntil { posts.any { it.first.endsWith("/conversations/c-run/cancel") } }
    }

    @Test
    fun aPermissionRequestIsAnsweredFromTheCard() {
        showBoard()

        rule.waitUntil { rule.onAllNodesWithText("Run npm test").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Allow once: Run npm test").performClick()

        rule.waitUntil { posts.any { it.first.endsWith("/conversations/c-input/permission") } }
        assertEquals("""{"request_id":"r1","option_id":"allow"}""", posts.first { it.first.endsWith("/permission") }.second)
    }

    @Test
    fun startingABacklogTaskStartsAConversationAndMovesTheCardToRunning() {
        showBoard()

        rule.onNodeWithContentDescription("Start Rebuild install.md").performClick()

        rule.waitUntil { posts.any { it.first.endsWith("/tasks/t1/start") } }
        rule.waitUntil { rule.onAllNodesWithText("Start", substring = false).fetchSemanticsNodes().isEmpty() }
        rule.onNodeWithContentDescription("Stop Rebuild install.md").assertExists()
    }

    @Test
    fun aTaskNeedsATitleAndAFolderBeforeItIsAdded() {
        showBoard()

        rule.onNodeWithText("Add a task").performClick()
        rule.onNodeWithText("Add task").performClick()

        rule.onNodeWithText("Enter a title for this task.").assertExists()
        rule.onNodeWithText("Enter the folder the agent should work in, for example /home/you/project.").assertExists()
        assertTrue(posts.none { it.first.endsWith("/tasks") })
    }

    @Test
    fun addingATaskSendsItsTitleFolderNodeAndAgent() {
        showBoard()

        rule.onNodeWithText("Add a task").performClick()
        rule.onNodeWithContentDescription("Title").performTextInput("Tidy")
        rule.onNodeWithContentDescription("Folder").performTextInput("/srv")
        rule.onNodeWithText("Add task").performClick()

        rule.waitUntil { posts.any { it.first.endsWith("/tasks") } }
        assertEquals("""{"node_id":"n1","agent":"claude","title":"Tidy","cwd":"/srv"}""", posts.first { it.first.endsWith("/tasks") }.second)
    }
}
