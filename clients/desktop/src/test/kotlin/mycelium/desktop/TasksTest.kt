package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
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

private fun conversation(id: String, title: String, status: String, cwd: String = "/other", agent: String = "claude", model: String = "null", project: String = "null") =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"$agent","cwd":"$cwd","title":"$title","status":"$status","acp_session_id":"s","mode":null,"model":$model,"project_id":$project,
"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val BoardNodes = """[{"id":"n1","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"v0.1.13","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2","available":true},{"agent":"codex","version":"1","available":true}],"logins":[],"sessions":[]}]"""

private fun task(id: String, title: String, status: String, extra: String = "") =
    """{"id":"$id","node_id":"n1","agent":"claude","cwd":"/work/app","project_id":"p1","branch":null,"title":"$title","status":"$status","created_at":"2026-10-22T10:00:00Z"$extra}"""

private val BoardTaskList = "[" + listOf(
    task("t1", "Rebuild install.md", "open"),
    task("tq", "Tag now, or wait?", "question", ""","options":["Tag now","Wait"],"source_title":"Release""""),
    task("tp", "Cache cargo in the release jobs", "proposed", ""","source_title":"Release""""),
    task("tc", "Merge the Gradle jobs", "claimed", ""","conversation_id":"c-claimed""""),
    task("tx", "Old idea", "declined"),
).joinToString(",") + "]"

private val BoardConversations = "[" + listOf(
    conversation("c-claimed", "Gradle merge", "review"),
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

    private fun store(tasks: String = BoardTaskList, conversations: String = BoardConversations): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post) {
                posts += path to String(request.body.toByteArray())
            }

            when {
                request.method == HttpMethod.Post && path.endsWith("/tasks/tp/approve") -> respond(task("tp", "Cache cargo in the release jobs", "open"), HttpStatusCode.OK, json)
                request.method == HttpMethod.Post && path.endsWith("/tasks/tp/decline") -> respond(task("tp", "Cache cargo in the release jobs", "declined"), HttpStatusCode.OK, json)
                request.method == HttpMethod.Post && path.endsWith("/tasks/tq/answer") -> respond(task("tq", "Tag now, or wait?", "done", ""","answer":"Wait""""), HttpStatusCode.OK, json)
                request.method == HttpMethod.Post && path.endsWith("/tasks/tc/done") -> respond(task("tc", "Merge the Gradle jobs", "done", ""","conversation_id":"c-claimed""""), HttpStatusCode.OK, json)
                request.method == HttpMethod.Post && path.endsWith("/tasks/t1/start") && posts.last().second.contains("c-idle") -> respond(conversation("c-idle", "Ready for a look", "working", "/work/app", project = "\"p1\""), HttpStatusCode.OK, json)
                request.method == HttpMethod.Post && path.endsWith("/tasks/t1/start") -> respond(conversation("c-new", "Rebuild install.md", "working", "/work/app"), HttpStatusCode.Created, json)
                request.method == HttpMethod.Post && path.endsWith("/tasks") -> respond("""{"id":"t2","node_id":"n1","agent":"claude","cwd":"/srv","title":"Tidy","created_at":"2026-10-22T10:00:00Z"}""", HttpStatusCode.Created, json)
                path.endsWith("/conversations/c-input/events") -> respond(PermissionEvent, HttpStatusCode.OK, json)
                path.endsWith("/events") || path.endsWith("/queue") -> respond("[]", HttpStatusCode.OK, json)
                path.endsWith("/tasks") -> respond(tasks, HttpStatusCode.OK, json)
                path.endsWith("/projects") -> respond("""[{"id":"p1","node_id":"n1","name":"app","path":"/work/app","created_at":"2026-10-22T10:00:00Z"}]""", HttpStatusCode.OK, json)
                path.endsWith("/worktrees") -> respond("""[{"id":"w1","project_id":"p1","branch":"feature/x","path":"/work/app-wt/feature-x","created_at":"2026-10-22T10:00:00Z"}]""", HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(BoardNodes, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond(conversations, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        runBlocking { store.refresh() }

        return store
    }

    private fun showBoard(store: Store = store(), onOpen: (String) -> Unit = {}, ready: String = "Rebuild install.md") {
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); TasksScreen(store, state, platform, onOpen) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText(ready).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun tasksSitInTheLaneOfTheirStatusAndConversationsWithoutATaskInClaimed() {
        showBoard()

        for (title in listOf("Tag now, or wait?", "Cache cargo in the release jobs", "Rebuild install.md", "Merge the Gradle jobs", "Waiting on you", "Running job", "Ready for a look", "Broke down")) {
            rule.onNodeWithText(title).assertExists()
        }

        for (lane in listOf("Questions", "Proposed", "Open", "Claimed")) {
            rule.onNodeWithText(lane).assertExists()
        }

        rule.onNodeWithText("Gradle merge").assertDoesNotExist()
        rule.onNodeWithText("Old idea").assertDoesNotExist()
    }

    @Test
    fun questionsAndProposalsNameWhereTheyCameFrom() {
        showBoard()

        rule.onNodeWithText("From Release").assertExists()
        rule.onNodeWithText("app · from Release").assertExists()
    }

    @Test
    fun approvingAProposalPostsItAndOffersItToStart() {
        showBoard()

        rule.onNodeWithContentDescription("Approve Cache cargo in the release jobs").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/tp/approve") } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Start Cache cargo in the release jobs").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Approve Cache cargo in the release jobs").assertDoesNotExist()
    }

    @Test
    fun decliningAProposalHidesItFromTheBoard() {
        showBoard()

        rule.onNodeWithContentDescription("Decline Cache cargo in the release jobs").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Cache cargo in the release jobs").fetchSemanticsNodes().isEmpty() }
        assertTrue(posts.any { it.first.endsWith("/tasks/tp/decline") })
    }

    @Test
    fun showDeclinedBringsDeclinedTasksBackIntoDone() {
        platform.saved["tasks-collapsed-lanes"] = ""
        showBoard()

        rule.onNodeWithText("Old idea").assertDoesNotExist()
        rule.onNodeWithText("Show declined").performClick()

        rule.onNodeWithText("Old idea").assertExists()
        rule.onNodeWithText("Declined").assertExists()
        rule.onNodeWithText("Hide declined").assertExists()
    }

    @Test
    fun aQuestionIsAnsweredByPickingAnOptionAndTheAnswerIsSent() {
        showBoard()

        rule.onNodeWithContentDescription("Answer Tag now, or wait?").performClick()
        rule.onNodeWithContentDescription("Answer Tag now, or wait?: Wait").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/tq/answer") } }
        assertEquals("""{"answer":"Wait"}""", posts.first { it.first.endsWith("/tasks/tq/answer") }.second)
    }

    @Test
    fun aQuestionIsAnsweredInYourOwnWordsAndAnEmptyAnswerIsRefused() {
        showBoard()

        rule.onNodeWithContentDescription("Answer Tag now, or wait?").performClick()
        rule.onNodeWithText("Send answer").performClick()
        rule.onNodeWithText("Enter an answer to send.").assertExists()
        assertTrue(posts.none { it.first.endsWith("/answer") })

        rule.onNodeWithContentDescription("Answer. Error: Enter an answer to send.").performTextInput("Tag it tomorrow")
        rule.onNodeWithText("Send answer").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/tq/answer") } }
        assertEquals("""{"answer":"Tag it tomorrow"}""", posts.first { it.first.endsWith("/tasks/tq/answer") }.second)
    }

    @Test
    fun aSpaceTypedIntoTheAnswerKeepsTheDialogOpenAndTheAnswerIsSent() {
        showBoard()

        rule.onNodeWithContentDescription("Answer Tag now, or wait?").performClick()
        rule.onNodeWithContentDescription("Answer").performTextInput("test reply")
        rule.onNodeWithContentDescription("Answer").performKeyInput {
            pressKey(Key.Spacebar)
            pressKey(Key.E)
        }

        rule.onNodeWithText("Answer a question").assertExists()
        rule.onNodeWithText("Send answer").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/tq/answer") } }
        assertEquals("""{"answer":"test reply"}""", posts.first { it.first.endsWith("/tasks/tq/answer") }.second)
    }

    @Test
    fun aClaimedTaskWhoseConversationIsReadyIsMarkedDoneFromItsCard() {
        showBoard()

        rule.onNodeWithText("Ready to review").assertExists()
        rule.onNodeWithContentDescription("Mark Merge the Gradle jobs done").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/tc/done") } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Mark Merge the Gradle jobs done").fetchSemanticsNodes().isEmpty() }
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
        platform.saved["tasks-collapsed-lanes"] = "Claimed,Done"
        showBoard()

        rule.onNodeWithText("Running job").assertDoesNotExist()
        rule.onNodeWithContentDescription("Expand Claimed, 5").assertExists()
        rule.onNodeWithText("Cache cargo in the release jobs").assertExists()
    }

    @Test
    fun collapsingALaneHidesItsCardsAndSavesTheCollapsedLanes() {
        showBoard()

        rule.onNodeWithContentDescription("Collapse Claimed").performScrollTo().performClick()

        rule.onNodeWithText("Running job").assertDoesNotExist()
        assertEquals(setOf("Done", "Claimed"), platform.saved.getValue("tasks-collapsed-lanes").split(',').toSet())
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

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/conversations/c-run/cancel") } }
    }

    @Test
    fun aPermissionRequestIsAnsweredFromTheCard() {
        showBoard()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Run npm test").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Allow once: Run npm test").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/conversations/c-input/permission") } }
        assertEquals("""{"request_id":"r1","option_id":"allow"}""", posts.first { it.first.endsWith("/permission") }.second)
    }

    @Test
    fun startingAnOpenTaskStartsAConversationAndClaimsTheCard() {
        showBoard()

        rule.onNodeWithContentDescription("Start Rebuild install.md").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/t1/start") } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Start", substring = false).fetchSemanticsNodes().isEmpty() }
        rule.onNodeWithContentDescription("Stop Rebuild install.md").assertExists()
    }

    private fun withIdleConversation(vararg others: String) = store(conversations = "[" + (listOf(conversation("c-idle", "Ready for a look", "review", "/work/app", project = "\"p1\"")) + others).joinToString(",") + "]")

    @Test
    fun anOpenTaskOffersAnyIdleConversationOfItsProject() {
        showBoard(withIdleConversation())

        rule.onNodeWithText("app · any idle conversation").assertExists()
    }

    @Test
    fun anOpenTaskKeepsItsOwnDetailsWhenNoConversationOfItsProjectIsIdle() {
        showBoard(store(conversations = "[" + conversation("c-working", "Busy", "working", "/work/app", project = "\"p1\"") + "," + conversation("c-other", "Elsewhere", "review", project = "\"p2\"") + "]"))

        rule.onNodeWithText("any idle conversation", substring = true).assertDoesNotExist()
        rule.onAllNodesWithText("Claude Code · app").fetchSemanticsNodes().isNotEmpty().let(::assertTrue)
    }

    @Test
    fun startingATaskWithIdleConversationsOffersNewAndEachConversation() {
        showBoard(withIdleConversation())

        rule.onNodeWithContentDescription("Start Rebuild install.md").performClick()

        rule.onNodeWithText("New conversation").assertExists()
        rule.onNodeWithText("Send to Ready for a look").assertExists()
        assertTrue(posts.none { it.first.endsWith("/tasks/t1/start") })
    }

    @Test
    fun startingATaskAlsoOffersWorkingConversationsOfItsProjectToQueueItInAndChats() {
        showBoard(withIdleConversation(conversation("c-busy", "Still working", "working", "/work/app", project = "\"p1\""), conversation("c-chat", "Planning chat", "review", agent = "chat"), conversation("c-busy-chat", "Replying chat", "working", agent = "chat"), conversation("c-elsewhere", "Other project", "working", project = "\"p2\"")))

        rule.onNodeWithContentDescription("Start Rebuild install.md").performClick()

        rule.onNodeWithText("Send to Ready for a look").assertExists()
        rule.onNodeWithText("Queue for Still working").assertExists()
        rule.onNodeWithText("Send to Planning chat").assertExists()
        rule.onNodeWithText("Queue for Replying chat").assertExists()
        rule.onNodeWithText("Queue for Other project").assertDoesNotExist()
    }

    @Test
    fun queueingATaskForAWorkingConversationPostsItsId() {
        showBoard(withIdleConversation(conversation("c-busy", "Still working", "working", "/work/app", project = "\"p1\"")))

        rule.onNodeWithContentDescription("Start Rebuild install.md").performClick()
        rule.onNodeWithText("Queue for Still working").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/t1/start") } }
        assertEquals("""{"conversation_id":"c-busy"}""", posts.first { it.first.endsWith("/tasks/t1/start") }.second)
    }

    @Test
    fun sendingATaskToAnIdleConversationPostsItsIdAndClaimsTheCard() {
        showBoard(withIdleConversation())

        rule.onNodeWithContentDescription("Start Rebuild install.md").performClick()
        rule.onNodeWithText("Send to Ready for a look").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/t1/start") } }
        assertEquals("""{"conversation_id":"c-idle"}""", posts.first { it.first.endsWith("/tasks/t1/start") }.second)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Start Rebuild install.md").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun choosingNewConversationFromTheStartMenuStartsOneWithoutATarget() {
        showBoard(withIdleConversation())

        rule.onNodeWithContentDescription("Start Rebuild install.md").performClick()
        rule.onNodeWithText("New conversation").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/t1/start") } }
        assertEquals("", posts.first { it.first.endsWith("/tasks/t1/start") }.second)
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

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks") } }
        assertEquals("""{"node_id":"n1","agent":"claude","title":"Tidy","cwd":"/srv"}""", posts.first { it.first.endsWith("/tasks") }.second)
    }

    private fun dependants(): String = "[" + listOf(
        task("tc", "Merge the Gradle jobs", "claimed", ""","conversation_id":"c-claimed""""),
        task("t3", "Verify v0.1.11 from S3", "open", ""","depends_on":["tc"]"""),
    ).joinToString(",") + "]"

    @Test
    fun aTaskThatWaitsOnAnotherSaysSoAndStartingItExplainsWhyItCannotStart() {
        showBoard(store(dependants()), ready = "Verify v0.1.11 from S3")

        rule.onNodeWithText("waits on 1 task", substring = true).assertExists()

        rule.onNodeWithContentDescription("Start Verify v0.1.11 from S3").performClick()

        rule.onNodeWithText("Finish Merge the Gradle jobs before starting this task.").assertExists()
        assertTrue(posts.none { it.first.endsWith("/tasks/t3/start") })
    }

    @Test
    fun aTaskBecomesStartableWhenTheTaskItWaitsOnIsDone() {
        showBoard(store(dependants()), ready = "Verify v0.1.11 from S3")

        rule.onNodeWithContentDescription("Mark Merge the Gradle jobs done").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("waits on 1 task", substring = true).fetchSemanticsNodes().isEmpty() }
        rule.onNodeWithContentDescription("Start Verify v0.1.11 from S3").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks/t3/start") } }
    }

    @Test
    fun aTaskIsAddedWithTheOpenTasksItWaitsOn() {
        showBoard()

        rule.onNodeWithText("Add a task").performClick()
        rule.onNodeWithContentDescription("Title").performTextInput("Tidy")
        rule.onNodeWithContentDescription("Folder").performTextInput("/srv")
        rule.onNodeWithContentDescription("Waits on Rebuild install.md").performClick()
        rule.onNodeWithText("Add task").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/tasks") } }
        assertEquals("""{"node_id":"n1","agent":"claude","title":"Tidy","cwd":"/srv","depends_on":["t1"]}""", posts.first { it.first.endsWith("/tasks") }.second)
    }
}
