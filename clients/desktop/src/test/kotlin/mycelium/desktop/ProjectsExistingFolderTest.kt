package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.ProjectsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Laptop = "aaaaaaaa-0000-0000-0000-000000000001"

private const val Node = """[{"id":"$Laptop","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]}]"""

private const val Forge = """[{"id":"f1","kind":"github","base_url":"https://github.com","account":"samexample","token_hint":"wxyz","organisations":["acme-org"],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val Repository = """{"repositories":[{"forge_id":"f1","owner":"acme-org","name":"backups","full_name":"acme-org/backups","private":false}],"organisations":[],"failures":[]}"""

private const val Created = """{"id":"p1","node_id":"$Laptop","name":"backups","path":"/home/sam/backups","forge_id":"f1","repository":"acme-org/backups","created_at":"2026-10-22T10:00:00Z"}"""

class ProjectsExistingFolderTest {
    @get:Rule
    val rule = createComposeRule()

    private val posts = mutableListOf<String>()
    private var answer = HttpStatusCode.Created to Created

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/projects") && request.method == HttpMethod.Post -> {
                    posts += String(request.body.toByteArray())
                    respond(answer.second, answer.first, json)
                }
                path.endsWith("/repositories") -> respond(Repository, HttpStatusCode.OK, json)
                path.endsWith("/projects") || path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                path.endsWith("/forges") -> respond(Forge, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(Node, HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("backups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Set up acme-org/backups on a node").performClick()
    }

    @Test
    fun aRepositoryCanBeClonedOntoANodeOrPointedAtAnExistingFolder() {
        show()

        rule.onNode(hasText("Clone onto a node") and hasClickAction()).assertExists()
        rule.onNode(hasText("Use an existing folder") and hasClickAction()).assertExists()
    }

    @Test
    fun theFolderIsSentWithTheRepositoryAndTheNodeAndTheDialogClosesWhenItIsAccepted() {
        show()

        rule.onNode(hasText("Use an existing folder") and hasClickAction()).performClick()
        rule.onNodeWithContentDescription("Folder on the node").performTextInput("/home/sam/backups")
        rule.onNode(hasText("Use folder") and hasClickAction()).performClick()
        rule.waitUntil(5_000) { posts.isNotEmpty() }

        assertEquals("""{"node_id":"$Laptop","forge_id":"f1","repository":"acme-org/backups","path":"/home/sam/backups"}""", posts.single().replace(",\"new_repository\":false", ""))
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Use folder").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun aFolderTheControlPlaneRefusesStaysOpenWithItsMessage() {
        answer = HttpStatusCode.BadRequest to """{"error":"folder_other_repository","message":"The folder at /x is a checkout of other/repo, not acme-org/backups. Choose the folder that holds acme-org/backups."}"""
        show()

        rule.onNode(hasText("Use an existing folder") and hasClickAction()).performClick()
        rule.onNodeWithContentDescription("Folder on the node").performTextInput("/x")
        rule.onNode(hasText("Use folder") and hasClickAction()).performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("is a checkout of other/repo", substring = true).fetchSemanticsNodes().isNotEmpty() }
    }
}
