package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.semantics.SemanticsProperties
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
import kotlinx.coroutines.runBlocking
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.Repository
import mycelium.state.Store
import mycelium.ui.ExistingFolderDialog
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewConversationScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val NodeId = "aaaaaaaa-0000-0000-0000-000000000001"

private const val NodesJson = """[{"id":"$NodeId","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.10.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[]}]"""

private const val RootListing = """{"root":"/work","path":"/work","parent":null,"directories":[
{"name":"backups","path":"/work/backups","repository":true},{"name":"notes","path":"/work/notes","repository":false}]}"""

private const val BackupsListing = """{"root":"/work","path":"/work/backups","parent":"/work","directories":[]}"""

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"$NodeId","agent":"claude","cwd":"/work/backups","title":"New","status":"working",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val ProjectJson = """{"id":"p1","node_id":"$NodeId","name":"backups","path":"/work/backups","forge_id":"f1","repository":"acme-org/backups","created_at":"2026-10-22T10:00:00Z"}"""

class FolderBrowserTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())
    private var directories: Pair<HttpStatusCode, String>? = null
    private var refusal: String? = null
    private val listed = java.util.Collections.synchronizedList(mutableListOf<String>())

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post) {
                posts += path to String(request.body.toByteArray())
            }

            if (path.endsWith("/directories") && request.method == HttpMethod.Get) {
                listed += request.url.parameters["path"].orEmpty()
            }

            when {
                path.endsWith("/directories") && request.method == HttpMethod.Post && refusal != null -> respond("""{"error":"git_failed","message":"$refusal"}""", HttpStatusCode.Conflict, json)
                path.endsWith("/directories") && request.method == HttpMethod.Post -> respond("""{"name":"quiet-otter","path":"/work/quiet-otter"}""", HttpStatusCode.Created, json)
                path.endsWith("/directories") -> {
                    val requested = request.url.parameters["path"].orEmpty()
                    val answer = directories ?: (HttpStatusCode.OK to if (requested == "/work/backups") BackupsListing else RootListing)

                    respond(answer.second, answer.first, json)
                }
                path.endsWith("/projects") && request.method == HttpMethod.Post -> respond(ProjectJson, HttpStatusCode.Created, json)
                path.endsWith("/conversations") && request.method == HttpMethod.Post -> respond(ConversationJson, HttpStatusCode.Created, json)
                path.endsWith("/nodes") -> respond(NodesJson, HttpStatusCode.OK, json)
                path.endsWith("/projects") || path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        return store
    }

    private val backups = Repository("f1", "acme-org", "backups", "acme-org/backups")

    @Test
    fun browsing_from_use_an_existing_folder_fills_the_folder_and_sets_up_the_project() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ExistingFolderDialog(store, state, backups, onDismiss = {}) } }

        rule.onNodeWithContentDescription("Browse folders on laptop").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("backups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Git").assertExists()
        rule.onNodeWithText("backups").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Up").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Choose this folder").performClick()
        rule.onNodeWithText("Use folder").performClick()

        rule.waitUntil(5_000) { posts.isNotEmpty() }
        assertEquals("""{"node_id":"$NodeId","forge_id":"f1","repository":"acme-org/backups","path":"/work/backups"}""", posts.single().second)
    }

    private fun browseFromTheExistingFolderDialog(store: Store) {
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ExistingFolderDialog(store, state, backups, onDismiss = {}) } }

        rule.onNodeWithContentDescription("Browse folders on laptop").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("notes").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun new_folder_offers_a_random_two_word_name_and_enter_creates_the_folder_on_the_node_and_opens_it() {
        val store = store()
        browseFromTheExistingFolderDialog(store)

        rule.onNodeWithText("New folder").performClick()
        val offered = rule.onNodeWithContentDescription("New folder name").fetchSemanticsNode().config[SemanticsProperties.EditableText].text
        assertTrue(Regex("[a-z]+-[a-z]+").matches(offered), offered)
        rule.onNodeWithContentDescription("New folder name").performTextReplacement("quiet-otter")
        rule.onNodeWithContentDescription("New folder name").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { "/work/quiet-otter" in listed }
        assertEquals("""{"path":"/work","name":"quiet-otter"}""", posts.single().second)
        assertEquals(0, rule.onAllNodesWithContentDescription("New folder name").fetchSemanticsNodes().size)
    }

    @Test
    fun escape_cancels_the_new_folder_name_without_creating_anything() {
        val store = store()
        browseFromTheExistingFolderDialog(store)

        rule.onNodeWithText("New folder").performClick()
        rule.onNodeWithContentDescription("New folder name").performKeyInput { pressKey(Key.Escape) }

        assertEquals(0, rule.onAllNodesWithContentDescription("New folder name").fetchSemanticsNodes().size)
        assertEquals(emptyList(), posts)
    }

    @Test
    fun a_name_the_node_refuses_is_shown_and_the_name_stays_for_editing() {
        val store = store()
        browseFromTheExistingFolderDialog(store)
        refusal = "quiet-otter already exists in /work. Choose another name."

        rule.onNodeWithText("New folder").performClick()
        rule.onNodeWithContentDescription("New folder name").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("quiet-otter already exists in /work. Choose another name.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("New folder name").assertExists()
    }

    @Test
    fun a_new_conversation_in_a_folder_can_be_started_from_a_browsed_folder() {
        val store = store()
        var created: Conversation? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewConversationScreen(store, state, onCreated = { created = it }) } }

        rule.onNodeWithContentDescription("Place: ", substring = true).performClick()
        rule.onNodeWithContentDescription("Browse folders on laptop").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("notes").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Choose this folder").performClick()
        rule.onNodeWithContentDescription("Send").performClick()

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$NodeId","agent":"claude","cwd":"/work"}""", posts.single().second)
    }

    @Test
    fun the_folder_can_still_be_typed() {
        val store = store()
        var created: Conversation? = null
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewConversationScreen(store, state, onCreated = { created = it }) } }

        rule.onNodeWithContentDescription("Place: ", substring = true).performClick()
        rule.onNodeWithContentDescription("Folder").performTextInput("/work/midden")
        rule.onNodeWithContentDescription("Folder").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { created != null }
        assertEquals("""{"node_id":"$NodeId","agent":"claude","cwd":"/work/midden"}""", posts.single().second)
    }

    @Test
    fun an_outdated_node_says_to_update_it_or_type_the_path() {
        directories = HttpStatusCode.Conflict to """{"error":"node_outdated","message":"Update laptop to browse its folders, or enter the path."}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NewConversationScreen(store, state, onCreated = {}) } }

        rule.onNodeWithContentDescription("Place: ", substring = true).performClick()
        rule.onNodeWithContentDescription("Browse folders on laptop").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Update laptop to browse its folders, or enter the path.").fetchSemanticsNodes().isNotEmpty() }
    }
}
