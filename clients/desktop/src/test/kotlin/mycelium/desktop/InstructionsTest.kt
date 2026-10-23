package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
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
import mycelium.ui.InstructionsSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val NodesJson = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[],"logins":[],"instructions_revision":"afad7763c4d5e6f708192a3b4c5d6e7f80910203","instructions_unmanaged":["Claude Code: /home/sam/.claude/CLAUDE.md"],"sessions_on_older_instructions":2},
{"id":"n2","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","online":false,"draining":false,
"agents":[],"logins":[],"instructions_revision":"85d49d3aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]"""

private const val DocumentJson = """{"source":{"forge_id":"f1","repository":"slop/agents-md","path":"AGENTS.md"},"revision":"afad7763c4d5e6f708192a3b4c5d6e7f80910203","layers":[
{"layer":"global","name":"Global","path":"AGENTS.md","text":"## Diffs","version":{"id":"afad7763c4d5e6f708192a3b4c5d6e7f80910203","created_at":"2026-10-22T10:00:00Z"}},
{"layer":"g1","name":"slop","path":"AGENTS.slop.md","text":"Use British English.","version":null},
{"layer":"g2","name":"work","path":"AGENTS.work.md","text":"","version":null}]}"""

private const val ProjectsJson = """[{"id":"p1","node_id":"n1","name":"mycelium","path":"/p/mycelium","group_id":null,"created_at":"2026-10-22T10:00:00Z"}]"""

class InstructionsTest {
    @get:Rule
    val rule = createComposeRule()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<Triple<HttpMethod, String, String>>())

    private fun requestTo(method: HttpMethod, pathEnd: String): Triple<HttpMethod, String, String> {
        rule.waitUntil(timeoutMillis = 5_000) { requests.toList().any { it.first == method && it.second.endsWith(pathEnd) } }

        return requests.toList().first { it.first == method && it.second.endsWith(pathEnd) }
    }

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += Triple(request.method, path, request.body.toByteArray().decodeToString())
            val body = when {
                path.endsWith("/conversations") -> "[]"
                path.endsWith("/nodes") -> NodesJson
                path.endsWith("/instructions") -> DocumentJson
                path.endsWith("/projects") -> ProjectsJson
                path.endsWith("/forges") -> "[]"
                path.contains("/projects/") -> """{"id":"p1","node_id":"n1","name":"mycelium","path":"/p/mycelium","group_id":"g1","created_at":"2026-10-22T10:00:00Z"}"""
                path.contains("/instructions/") -> """{"layer":"global","name":"Global","text":"x"}"""
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { InstructionsSettings(store) } }
    }

    @Test
    fun theGlobalFileShowsItsSourceCommitAndTextWithAGroupTabBesideIt() {
        show()

        rule.onNodeWithText("agents-md / AGENTS.md").assertExists()
        rule.onNodeWithText("afad776 · ", substring = true).assertExists()
        rule.onNodeWithContentDescription("Global instructions").assertExists()
        rule.onNodeWithContentDescription("Instructions for the slop group").assertExists()
        rule.onNodeWithContentDescription("Instructions for the work group").assertExists()
        rule.onNodeWithText("## Diffs").assertExists()
    }

    @Test
    fun aGroupTabShowsItsOwnLayerFileAndSaysNoVersionYet() {
        show()

        rule.onNodeWithContentDescription("Instructions for the slop group").performClick()

        rule.onNodeWithText("agents-md / AGENTS.slop.md").assertExists()
        rule.onNodeWithText("No version yet").assertExists()
        rule.onNodeWithText("Use British English.").assertExists()
    }

    @Test
    fun eachNodeShowsTheVersionItHoldsAndItsSessionsOnAnOlderVersion() {
        show()

        rule.onNodeWithText("afad776").assertExists()
        rule.onNodeWithText("2 sessions").assertExists()
        rule.onNodeWithText("85d49d3").assertExists()
        rule.onNodeWithText("Offline, catches up on reconnect").assertExists()
    }

    @Test
    fun aFileTheNodeDoesNotManageIsNamedAsLeftAsItIs() {
        show()

        rule.onNodeWithText("Not managed, left as it is: Claude Code: /home/sam/.claude/CLAUDE.md").assertExists()
    }

    @Test
    fun savingSendsTheEditedTextForTheLayerThatIsOpen() {
        show()

        rule.onNodeWithContentDescription("Global instructions").performClick()
        rule.onNodeWithText("Save and commit").performClick()

        val saved = requestTo(HttpMethod.Put, "/instructions/global")
        assertEquals("""{"text":"## Diffs"}""", saved.third)
    }

    @Test
    fun aProjectIsAddedToTheOpenGroup() {
        show()

        rule.onNodeWithContentDescription("Instructions for the slop group").performClick()
        rule.onNodeWithContentDescription("Add mycelium to slop").performClick()

        val patched = requestTo(HttpMethod.Patch, "/projects/p1")
        assertEquals("""{"group_id":"g1"}""", patched.third)
    }

    @Test
    fun aNewGroupIsCreatedFromItsNameAndTheDocumentIsReloaded() {
        show()

        rule.onNodeWithContentDescription("New group").performClick()
        rule.onNodeWithContentDescription("Group name").performTextInput("homelab")
        rule.onNodeWithText("Create group").performClick()

        val created = requestTo(HttpMethod.Post, "/groups")
        assertEquals("""{"name":"homelab"}""", created.third)
    }
}
