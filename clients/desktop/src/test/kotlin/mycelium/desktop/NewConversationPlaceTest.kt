package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewConversationScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val OrionId = "aaaaaaaa-0000-0000-0000-000000000001"

private const val OrionJson = """[{"id":"$OrionId","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[],"harness_models":[]}]"""

private const val HarbourJson = """[{"id":"p1","node_id":"$OrionId","name":"harbour","path":"/p/harbour","forge_id":null,"repository":null,"created_at":"2026-10-22T10:00:00Z"},
{"id":"p2","node_id":"$OrionId","name":"atlas","path":"/p/atlas","forge_id":null,"repository":null,"created_at":"2026-10-22T10:00:00Z"}]"""

private const val CreatedJson = """{"id":"c1","owner":"u","node_id":"$OrionId","agent":"claude","cwd":"/p/harbour","title":"New","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val RecentJson = """[{"id":"c0","owner":"u","node_id":"$OrionId","agent":"claude","cwd":"/p/harbour","title":"Restore flow","status":"done","project_id":"p1",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T11:00:00Z","archived_at":null}]"""

class NewConversationPlaceTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val bodies = CopyOnWriteArrayList<String>()

    private fun store(nodes: String = OrionJson): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/conversations") && request.method == HttpMethod.Post -> {
                    bodies += String(request.body.toByteArray())
                    respond(CreatedJson, HttpStatusCode.Created, json)
                }
                path.endsWith("/projects") -> respond(HarbourJson, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(nodes, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond(RecentJson, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.refreshProjects() }

        return store
    }

    private fun show(store: Store, onCreated: (Conversation) -> Unit = {}, onOpenNodes: () -> Unit = {}) {
        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                NewConversationScreen(store, state, onCreated = onCreated, onOpenNodes = onOpenNodes)
            }
        }
    }

    @Test
    fun theComposerStartsInTheMostRecentProjectAndOneEnterSendsTheFirstMessage() {
        var created: Conversation? = null
        show(store(), onCreated = { created = it })

        rule.onNodeWithContentDescription("Project: harbour").assertExists()
        rule.onNodeWithContentDescription("Place: orion, existing checkout").assertExists()
        rule.onNodeWithContentDescription("Message").performTextInput("Restore the copy")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$OrionId","agent":"claude","project_id":"p1","prompt":"Restore the copy"}""", bodies.single())
    }

    @Test
    fun theProjectMenuListsRecentProjectsBeforeAllProjects() {
        show(store())

        rule.onNodeWithContentDescription("Project: harbour").performClick()

        rule.onNodeWithText("Recent projects").assertExists()
        rule.onNodeWithText("All projects").assertExists()
        rule.onNodeWithText("atlas").assertExists()
        rule.onNodeWithText("Set up a project").assertExists()
    }

    @Test
    fun choosingNoProjectAndAFolderStartsThereWithoutAProject() {
        var created: Conversation? = null
        show(store(), onCreated = { created = it })

        rule.onNodeWithContentDescription("Project: harbour").performClick()
        rule.onNodeWithText("No project, choose a folder").performClick()
        rule.onNodeWithContentDescription("Place: orion, choose a folder").performClick()
        rule.onNodeWithContentDescription("Folder").performTextInput("/srv/sites/willow-site")
        rule.onNodeWithContentDescription("Place: orion, folder willow-site").assertExists()
        rule.onNodeWithContentDescription("Folder").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$OrionId","agent":"claude","cwd":"/srv/sites/willow-site"}""", bodies.single())
    }

    @Test
    fun sendingWithoutAFolderAsksForOneAndSendsNothing() {
        show(store())

        rule.onNodeWithContentDescription("Project: harbour").performClick()
        rule.onNodeWithText("No project, choose a folder").performClick()
        rule.onNodeWithContentDescription("Send").performClick()

        rule.onNodeWithText("Enter the folder the agent should work in", substring = true).assertExists()
        assertEquals(0, bodies.size)
    }

    @Test
    fun aNewWorktreeNamesItsBranchOnThePlaceControlAndSendsIt() {
        var created: Conversation? = null
        show(store(), onCreated = { created = it })

        rule.onNodeWithContentDescription("Place: orion, existing checkout").performClick()
        rule.onNodeWithText("New worktree").performClick()
        rule.onNodeWithContentDescription("New branch").performTextInput("restore-copy")
        rule.onNodeWithContentDescription("Place: orion, new worktree").assertExists()
        rule.onNodeWithContentDescription("New branch").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$OrionId","agent":"claude","project_id":"p1","branch":"restore-copy"}""", bodies.single())
    }

    @Test
    fun withNoNodeOnlineTheComposerStaysUsableAndTheNoticeLinksToNodes() {
        var openedNodes = false
        val store = store(nodes = "[]")
        show(store, onOpenNodes = { openedNodes = true })

        rule.onNodeWithText("No node is online.", substring = true).assertExists()
        rule.onNodeWithContentDescription("Message").performTextInput("Restore the copy")
        rule.onNodeWithContentDescription("Send").performClick()
        rule.onNodeWithContentDescription("Nodes").performClick()

        assertEquals(true, openedNodes)
        assertEquals(0, bodies.size)
        rule.onNodeWithText("Restore the copy").assertExists()
    }
}
