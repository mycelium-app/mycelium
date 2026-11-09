package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
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
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewConversationScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val OrionId = "aaaaaaaa-0000-0000-0000-000000000001"

private const val OrionJson = """[{"id":"$OrionId","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[],"harness_models":[]}]"""

private val OfflineOrionJson = OrionJson.replace("\"online\":true", "\"online\":false")

private const val HarbourJson = """[{"id":"p1","node_id":"$OrionId","name":"harbour","path":"/p/harbour","forge_id":null,"repository":null,"created_at":"2026-10-22T10:00:00Z"},
{"id":"p2","node_id":"$OrionId","name":"atlas","path":"/p/atlas","forge_id":null,"repository":null,"created_at":"2026-10-22T10:00:00Z"}]"""

private const val CreatedJson = """{"id":"c1","owner":"u","node_id":"$OrionId","agent":"claude","cwd":"/p/harbour","title":"New","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val RecentJson = """[{"id":"c0","owner":"u","node_id":"$OrionId","agent":"claude","cwd":"/p/harbour","title":"Restore flow","status":"done","project_id":"p1",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T11:00:00Z","archived_at":null}]"""

private const val TwoAccountsJson = """[{"id":"$OrionId","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true},{"agent":"codex","version":"0.9.0","available":true}],
"logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in"},{"agent":"claude","account":"work@example.org","method":"token","state":"signed_in"},{"agent":"codex","account":"sam@example.com","method":"token","state":"signed_in"}],
"harness_models":[{"agent":"claude","models":[{"id":"opus","name":"Opus"}],"default_model":"opus","reported_at":"2026-10-22T10:00:00Z"},{"agent":"codex","models":[{"id":"sol","name":"Sol"}],"default_model":"sol","reported_at":"2026-10-22T10:00:00Z"}]}]"""

private const val HarnessFirstSettings = """{"default_modes":{},"account_modes":[],"any_model":[{"agent":"claude"},{"agent":"codex","model":"sol"}]}"""

class NewConversationPlaceTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val bodies = CopyOnWriteArrayList<String>()
    private val configs = CopyOnWriteArrayList<String>()

    private fun store(nodes: String = OrionJson, settings: String = "{}"): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/conversations") && request.method == HttpMethod.Post -> {
                    bodies += String(request.body.toByteArray())
                    respond(CreatedJson, HttpStatusCode.Created, json)
                }
                path.endsWith("/config") -> {
                    configs += String(request.body.toByteArray())
                    respond("", HttpStatusCode.NoContent)
                }
                path.endsWith("/projects") -> respond(HarbourJson, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(nodes, HttpStatusCode.OK, json)
                path.endsWith("/agent-settings") -> respond(settings, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond(RecentJson, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.refreshProjects() }

        return store
    }

    private fun show(store: Store, onCreated: (Conversation) -> Unit = {}, onOpenNodes: () -> Unit = {}, phone: Boolean = false) {
        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                CompositionLocalProvider(LocalTouch provides phone) {
                    NewConversationScreen(store, state, onCreated = onCreated, onOpenNodes = onOpenNodes)
                }
            }
        }
    }

    @Test
    fun theComposerStartsInTheMostRecentProjectAndOneEnterSendsTheFirstMessage() {
        var created: Conversation? = null
        show(store(), onCreated = { created = it })

        rule.onNodeWithContentDescription("Project: harbour").assertExists()
        rule.onNodeWithContentDescription("Work in the existing checkout").assertExists()
        rule.onNodeWithContentDescription("Message").performTextInput("Restore the copy")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$OrionId","agent":"claude","project_id":"p1","prompt":"Restore the copy"}""", bodies.single())
    }

    @Test
    fun aFirstFavouriteThatNamesOnlyClaudeCodeStartsOnClaudeCodeWithAnyOfItsAccounts() {
        var created: Conversation? = null
        show(store(TwoAccountsJson, HarnessFirstSettings), onCreated = { created = it })

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Model: Claude Code", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Message").performTextInput("Plan the restore")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null && bodies.isNotEmpty() }
        assertTrue(bodies.single().contains("\"agent\":\"claude\""), bodies.single())
        assertTrue(bodies.single().contains("\"routing\":\"claude_any\""), bodies.single())
    }

    @Test
    fun aModeChosenBeforeSendingIsAppliedToTheNewConversation() {
        var created: Conversation? = null
        show(store(), onCreated = { created = it })

        rule.onNodeWithContentDescription("Mode: Manual").performClick()
        rule.onNodeWithText("Plan").performClick()
        rule.onNodeWithContentDescription("Message").performTextInput("Plan the restore")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null && bodies.isNotEmpty() }
        assertTrue(bodies.single().contains("\"mode\":\"plan\""))
        assertTrue(bodies.single().contains("\"prompt\":\"Plan the restore\""))
        assertEquals(0, configs.size)
    }

    @Test
    fun withoutChoosingAModeNoneIsSent() {
        var created: Conversation? = null
        show(store(), onCreated = { created = it })

        rule.onNodeWithContentDescription("Message").performTextInput("Restore the copy")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null }
        rule.waitForIdle()
        assertEquals(0, configs.size)
        assertTrue(!bodies.single().contains("\"mode\""))
    }

    @Test
    fun theProjectIsChosenInTheHeadlineAndReplacesItsName() {
        show(store())

        rule.onNodeWithText("What should we build in ", substring = true).assertExists()
        rule.onNodeWithContentDescription("Project: harbour").performClick()
        rule.onNodeWithText("atlas").performClick()

        rule.onNodeWithContentDescription("Project: atlas").assertExists()
        rule.onNodeWithContentDescription("Project: harbour").assertDoesNotExist()
    }

    @Test
    fun startingWithoutAProjectFromTheHeadlineAsksForAFolder() {
        show(store())

        rule.onNodeWithText("or start without a project").performClick()

        rule.onNodeWithContentDescription("Project: No project").assertExists()
        rule.onNodeWithContentDescription("Folder: choose a folder").assertExists()
        rule.onNodeWithText("or start without a project").assertDoesNotExist()
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
        rule.onNodeWithContentDescription("Folder: choose a folder").performClick()
        rule.onNodeWithContentDescription("Folder").performTextInput("/srv/sites/willow-site")
        rule.onNodeWithContentDescription("Folder: /srv/sites/willow-site").assertExists()
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

        rule.onNodeWithContentDescription("Work in the existing checkout").performClick()
        rule.onNodeWithText("New worktree").performClick()
        rule.onNodeWithContentDescription("New branch").performTextInput("restore-copy")
        rule.onNodeWithContentDescription("Work in a new worktree").assertExists()
        rule.onNodeWithContentDescription("New branch").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$OrionId","agent":"claude","project_id":"p1","branch":"restore-copy"}""", bodies.single())
    }

    @Test
    fun withNoNodeOnlineTheComposerStaysUsableAndTheNoticeLinksToNodes() {
        var openedNodes = false
        val store = store(nodes = "[]")
        show(store, onOpenNodes = { openedNodes = true })

        rule.onNodeWithText("No node is online.").assertExists()
        rule.onNodeWithText("Your message stays here.", substring = true).assertDoesNotExist()
        rule.onNodeWithContentDescription("Message").performTextInput("Restore the copy")
        rule.onNodeWithContentDescription("Send").performClick()
        rule.onNodeWithContentDescription("Nodes").performClick()

        assertEquals(true, openedNodes)
        assertEquals(0, bodies.size)
        rule.onNodeWithText("Restore the copy").assertExists()
    }

    @Test
    fun theLastKnownNodeShowsAsOfflineOnThePlaceControlWhenNoNodeIsOnline() {
        var openedNodes = false
        show(store(nodes = OfflineOrionJson), onOpenNodes = { openedNodes = true })

        rule.onNodeWithContentDescription("Place: orion is offline").performClick()

        assertEquals(true, openedNodes)
    }

    @Test
    fun onAPhoneTheProjectIsInTheHeadlineAndThePlaceStripSitsWithAttachAndSend() {
        show(store(), phone = true)

        rule.onNodeWithContentDescription("Place").assertExists()
        rule.onNodeWithContentDescription("Project: harbour").assertExists()
        rule.onNodeWithContentDescription("Work in the existing checkout").assertExists()
        rule.onNodeWithContentDescription("Attach files").assertExists()
        rule.onNodeWithContentDescription("Send").assertExists()
    }

    @Test
    fun browseInThePlaceMenuOpensTheFolderBrowserAndCancelReturns() {
        show(store())

        rule.onNodeWithContentDescription("Project: harbour").performClick()
        rule.onNodeWithText("No project, choose a folder").performClick()
        rule.onNodeWithContentDescription("Folder: choose a folder").performClick()
        rule.onNodeWithContentDescription("Browse folders on orion").performClick()

        rule.onNodeWithText("Choose a folder on orion").assertExists()
        rule.onNodeWithText("Cancel").performClick()
        rule.onNodeWithContentDescription("Folder: choose a folder").assertExists()
    }
}
