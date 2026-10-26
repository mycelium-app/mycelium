package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.input.key.Key
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
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.Project
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewConversationScreen
import mycelium.ui.NewProjectDialog
import mycelium.ui.ProjectsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val LaptopId = "aaaaaaaa-0000-0000-0000-000000000001"
private const val AtlasId = "aaaaaaaa-0000-0000-0000-000000000002"

private const val NodesJson = """[
{"id":"$LaptopId","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[]},
{"id":"$AtlasId","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[]}]"""

private const val ForgesJson = """[{"id":"f1","kind":"github","base_url":"https://github.com","account":"samexample","token_hint":"wxyz","organisations":["acme-org"],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val RepositoriesJson = """{"repositories":[
{"forge_id":"f1","owner":"acme-org","name":"acme-backups","full_name":"acme-org/acme-backups","private":true,"default_branch":"main"},
{"forge_id":"f1","owner":"immich-app","name":"immich","full_name":"immich-app/immich","private":false,"default_branch":"main"}],
"organisations":[{"forge_id":"f1","name":"acme-org"}],"failures":[]}"""

private const val BackupsProject = """{"id":"p1","node_id":"$LaptopId","name":"acme-backups","path":"/p/acme-backups","forge_id":"f1","repository":"acme-org/acme-backups","created_at":"2026-10-22T10:00:00Z"}"""

private const val FolderProject = """{"id":"p2","node_id":"$AtlasId","name":"homelab","path":"/p/homelab","forge_id":null,"repository":null,"created_at":"2026-10-22T10:00:00Z"}"""

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"$LaptopId","agent":"claude","cwd":"/p/acme-backups","title":"New","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

class ProjectsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())
    private val patches = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())
    private var repositoriesDelay = 20L
    private var groups = "[]"
    private var forges = ForgesJson
    private var repositories = RepositoriesJson
    private var projects = "[$BackupsProject,$FolderProject]"
    private var createProjectAnswer: Pair<HttpStatusCode, String> = HttpStatusCode.Created to BackupsProject

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = String(request.body.toByteArray())

            if (request.method == HttpMethod.Post) {
                posts += path to body
            }

            if (request.method == HttpMethod.Patch) {
                patches += path to body
            }

            if (path.endsWith("/repositories")) {
                delay(repositoriesDelay)
            }

            when {
                request.method == HttpMethod.Patch && path.contains("/projects/") -> respond(BackupsProject.replace("\"created_at\"", "\"group_id\":\"g1\",\"created_at\""), HttpStatusCode.OK, json)
                path.endsWith("/groups") -> respond(groups, HttpStatusCode.OK, json)
                path.endsWith("/projects") && request.method == HttpMethod.Post -> respond(createProjectAnswer.second, createProjectAnswer.first, json)
                path.endsWith("/conversations") && request.method == HttpMethod.Post -> respond(ConversationJson, HttpStatusCode.Created, json)
                path.endsWith("/projects") -> respond(projects, HttpStatusCode.OK, json)
                path.endsWith("/repositories") -> respond(repositories, HttpStatusCode.OK, json)
                path.endsWith("/forges") -> respond(forges, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(NodesJson, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        return store
    }

    @Test
    fun thePageListsRepositoriesWithTheNodesTheyAreSetUpOnAndRegisteredFolders() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("immich").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("acme-backups").assertExists()
        rule.onNodeWithText("acme-org").assertExists()
        rule.onAllNodesWithText("GitHub").fetchSemanticsNodes().let { assertEquals(2, it.size) }
        rule.onNodeWithContentDescription("Set up on laptop").assertExists()
        rule.onNodeWithText("laptop").assertExists()
        rule.onNodeWithText("homelab").assertExists()
        rule.onNodeWithContentDescription("Set up on atlas").assertExists()
    }

    @Test
    fun the_projects_tab_shows_the_last_repository_listing_before_the_forges_answer() {
        val store = store()
        runBlocking { store.refreshRepositories() }
        repositoriesDelay = 60_000
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } }

        rule.onNodeWithText("immich").assertExists()
        rule.onNodeWithContentDescription("Set up on laptop").assertExists()
    }

    @Test
    fun newConversationOpensTheProjectSetUpForThatRepository() {
        val store = store()
        var opened: String? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = { opened = it }) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("immich").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("New conversation in acme-org/acme-backups").performClick()

        assertEquals("p1", opened)
    }

    @Test
    fun withoutAnyForgeThePageSaysToConnectOneInSettings() {
        forges = "[]"
        repositories = """{"repositories":[],"organisations":[],"failures":[]}"""
        projects = "[]"
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Settings, Forges", substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aForgeThatFailedIsReportedWithItsMessage() {
        repositories = """{"repositories":[],"organisations":[],"failures":[{"forge_id":"f1","message":"GitHub did not answer. Try again in a moment."}]}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("GitHub did not answer. Try again in a moment.").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun cloningARepositorySendsTheNodeForgeAndRepositoryAndOpensTheProject() {
        val store = store()
        var created: String? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = { created = it.id }) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("acme-org/acme-backups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("acme-org/acme-backups").performClick()
        rule.onNodeWithContentDescription("Set up on atlas").performClick()
        rule.onNodeWithText("Clone and open").performClick()

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$AtlasId","forge_id":"f1","repository":"acme-org/acme-backups"}""", posts.single().second)
        assertEquals("p1", created)
    }

    @Test
    fun cloningIntoAGroupFilesTheNewProjectUnderIt() {
        groups = """[{"id":"g1","name":"work","created_at":"2026-10-22T10:00:00Z"},{"id":"g2","name":"slop","created_at":"2026-10-22T10:00:00Z"}]"""
        val store = store()
        var created: Project? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = { created = it }) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("acme-org/acme-backups").fetchSemanticsNodes().isNotEmpty() }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Group work").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("acme-org/acme-backups").performClick()
        rule.onNodeWithContentDescription("Group work").performClick()
        rule.onNodeWithText("Clone and open").performClick()

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"group_id":"g1"}""", patches.single().second)
        assertEquals("g1", created?.groupId)
    }

    @Test
    fun cloningWithoutAGroupLeavesTheProjectUngrouped() {
        groups = """[{"id":"g1","name":"work","created_at":"2026-10-22T10:00:00Z"}]"""
        val store = store()
        var created: Project? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = { created = it }) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("acme-org/acme-backups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("acme-org/acme-backups").performClick()
        rule.onNodeWithText("Clone and open").performClick()

        rule.waitUntil(5_000) { created != null }
        assertEquals(0, patches.size)
    }

    @Test
    fun registeringAFolderSendsTheNodeAndPath() {
        createProjectAnswer = HttpStatusCode.Created to FolderProject
        val store = store()
        var created: String? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = { created = it.id }) } }

        rule.onNodeWithText("A folder on a node").performClick()
        rule.onNodeWithContentDescription("Folder on the node").performTextInput("work/homelab")
        rule.onNodeWithText("Register and open").performClick()

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$AtlasId","path":"work/homelab"}""", posts.single().second)
    }

    @Test
    fun theChosenRepositoryFillsTheSearchFieldAndTheFolderItWillLiveInIsShown() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("acme-org/acme-backups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("acme-org/acme-backups").performClick()

        rule.onNodeWithText("acme-org/acme-backups").assertExists()
        assertEquals(0, rule.onAllNodesWithText("immich-app/immich").fetchSemanticsNodes().size)
        rule.onNodeWithText("~/projects/acme-org/acme-backups").assertExists()
    }

    @Test
    fun theStepsAreNumbered() {
        groups = """[{"id":"g1","name":"work","created_at":"2026-10-22T10:00:00Z"}]"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Group work").fetchSemanticsNodes().isNotEmpty() }

        listOf("1", "2", "3").forEach { rule.onNodeWithText(it).assertExists() }
    }

    @Test
    fun aNewRepositoryIsCreatedOnTheForgeThenClonedAsTheProject() {
        val store = store()
        var created: String? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = { created = it.id }) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set up on laptop").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("A new repository").performClick()
        rule.onNodeWithContentDescription("New repository").performTextInput("acme-org/restore-tool")
        rule.onNodeWithText("~/projects/acme-org/restore-tool").assertExists()
        rule.onNodeWithText("Create and open").performClick()

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$AtlasId","forge_id":"f1","repository":"acme-org/restore-tool","new_repository":true}""", posts.single().second)
    }

    @Test
    fun aNewRepositoryNeedsAnOwnerAndAName() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set up on laptop").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("A new repository").performClick()
        rule.onNodeWithContentDescription("New repository").performTextInput("restore-tool")
        rule.onNodeWithText("Create and open").performClick()

        rule.onNodeWithText("Enter the new repository as owner/name, for example acme-org/restore-tool.").assertExists()
        assertEquals(0, posts.size)
    }

    @Test
    fun aRefusedCloneShowsGitsMessageAndKeepsTheDialogOpen() {
        createProjectAnswer = HttpStatusCode.Conflict to """{"error":"git_failed","message":"repository not found"}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = {}) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("acme-org/acme-backups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("acme-org/acme-backups").performClick()
        rule.onNodeWithText("Clone and open").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("repository not found").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Clone and open").assertExists()
    }

    @Test
    fun cloningWithoutChoosingARepositoryAsksForOne() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewProjectDialog(store, state, onDismiss = {}, onCreated = {}) } }

        rule.onNodeWithText("Clone and open").performClick()

        rule.onNodeWithText("Choose the repository to clone.").assertExists()
        assertEquals(0, posts.size)
    }

    @Test
    fun aConversationStartedFromAProjectSendsProjectIdAndNoFolder() {
        val store = store()
        var created: Conversation? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewConversationScreen(store, state, onCreated = { created = it }, projectId = "p1") } }

        rule.waitUntil(5_000) { store.state.value.projects.isNotEmpty() }
        rule.waitForIdle()
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$LaptopId","agent":"claude","project_id":"p1"}""", posts.single().second)
    }

    @Test
    fun withoutAProjectTheFolderFallbackStillSendsTheFolder() {
        val store = store()
        var created: Conversation? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewConversationScreen(store, state, onCreated = { created = it }) } }

        rule.onNodeWithContentDescription("Folder").performTextInput("/work/midden")
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$AtlasId","agent":"claude","cwd":"/work/midden"}""", posts.single().second)
    }

    @Test
    fun theNewConversationScreenIsAHeadingOverOneComposerWithAttachAndTheAgentInsideIt() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { androidx.compose.runtime.CompositionLocalProvider(mycelium.ui.LocalPlatform provides QuietPlatform) { val state by store.state.collectAsState(); NewConversationScreen(store, state, onCreated = {}) } } }

        rule.onNodeWithText("What are we working on?").assertIsDisplayed()
        rule.onNodeWithContentDescription("Message").assertIsDisplayed()
        rule.onNodeWithContentDescription("Attach files").assertExists()
        rule.onNodeWithContentDescription("Agent: Claude Code").assertIsDisplayed()
        rule.onNodeWithContentDescription("Send").assertIsDisplayed()
    }

    @Test
    fun enterInTheComposerStartsTheConversationWithTheFirstMessage() {
        val store = store()
        var created: Conversation? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewConversationScreen(store, state, onCreated = { created = it }) } }

        rule.onNodeWithContentDescription("Folder").performTextInput("/work/midden")
        rule.onNodeWithContentDescription("Message").performTextInput("Fix the build")
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$AtlasId","agent":"claude","cwd":"/work/midden","prompt":"Fix the build"}""", posts.single().second)
    }
}
