package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewConversationScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val LaptopId = "aaaaaaaa-0000-0000-0000-000000000001"

private const val LaptopJson = """[{"id":"$LaptopId","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[],
"harness_models":[{"agent":"claude","models":[{"id":"opus","name":"Opus 5.5"},{"id":"sonnet","name":"Sonnet 5.5"}],"default_model":"opus","reported_at":"2026-10-22T08:00:00Z"}]}]"""

private const val ProjectJson = """[{"id":"p1","node_id":"$LaptopId","name":"app","path":"/p/app","forge_id":null,"repository":null,"created_at":"2026-10-22T10:00:00Z"}]"""

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"$LaptopId","agent":"claude","cwd":"/p/app-worktrees/x","title":"New","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

class WorktreesTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val bodies = CopyOnWriteArrayList<String>()
    private var conversationAnswer: Pair<HttpStatusCode, String> = HttpStatusCode.Created to ConversationJson

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/conversations") && request.method == HttpMethod.Post -> {
                    bodies += String(request.body.toByteArray())
                    respond(conversationAnswer.second, conversationAnswer.first, json)
                }
                path.endsWith("/projects") -> respond(ProjectJson, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(LaptopJson, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.refreshProjects() }

        return store
    }

    private fun show(store: Store, onCreated: () -> Unit = {}) {
        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                NewConversationScreen(store, state, onCreated = { onCreated() }, projectId = "p1")
            }
        }
    }

    private fun chooseNewWorktree() {
        rule.onNodeWithContentDescription("Place: ", substring = true).performClick()
        rule.onNodeWithText("New worktree").performClick()
    }

    @Test
    fun theBranchFieldAppearsOnlyWhenNewWorktreeIsOn() {
        show(store())

        assertEquals(0, rule.onAllNodesWithText("New branch").fetchSemanticsNodes().size)

        chooseNewWorktree()

        rule.onNodeWithContentDescription("New branch").assertExists()
    }

    @Test
    fun startingInANewWorktreeSendsTheProjectAndTheTrimmedBranch() {
        val store = store()
        var created = false
        show(store) { created = true }

        chooseNewWorktree()
        rule.onNodeWithContentDescription("New branch").performTextInput("  feature/restore ")
        rule.onNodeWithContentDescription("New branch").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created }
        assertEquals("""{"node_id":"$LaptopId","agent":"claude","project_id":"p1","branch":"feature/restore"}""", bodies.single())
    }

    @Test
    fun anEmptyBranchNameIsRefusedBeforeAnythingIsSent() {
        show(store())

        chooseNewWorktree()
        rule.onNodeWithContentDescription("New branch").performClick()
        rule.onNodeWithContentDescription("New branch").performKeyInput { pressKey(Key.Enter) }

        rule.onNodeWithText("Enter a name for the new branch.").assertExists()
        assertEquals(0, bodies.size)
    }

    @Test
    fun aRefusedBranchShowsGitsMessage() {
        conversationAnswer = HttpStatusCode.Conflict to """{"error":"git_failed","message":"a branch named 'x' already exists"}"""
        val store = store()
        show(store)

        chooseNewWorktree()
        rule.onNodeWithContentDescription("New branch").performTextInput("x")
        rule.onNodeWithContentDescription("New branch").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { store.problem.value != null }
        assertEquals("a branch named 'x' already exists", store.problem.value)
    }

    @Test
    fun theBranchHintNamesTheFolderTheWorktreeWillBeCreatedIn() {
        show(store())

        chooseNewWorktree()

        rule.onNodeWithContentDescription("New branch").performTextInput("fix/restore-copy")

        rule.onNodeWithText("Folder app-worktrees/fix-restore-copy").assertExists()
    }

    @Test
    fun theModelMenuOffersTheDefaultAndTheListTheNodeReportedAndTheChoiceIsSent() {
        val store = store()
        var created = false
        show(store) { created = true }

        rule.onNodeWithContentDescription("Model: Default model").performClick()
        rule.onNodeWithText("Claude Code on laptop, ", substring = true).assertExists()
        rule.onNodeWithText("Sonnet 5.5").performClick()
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil(5_000) { created }
        assertEquals("""{"node_id":"$LaptopId","agent":"claude","project_id":"p1","model":"sonnet"}""", bodies.single())
    }
}
