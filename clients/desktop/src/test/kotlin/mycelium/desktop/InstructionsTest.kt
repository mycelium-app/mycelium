package mycelium.desktop

import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.onParent
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.foundation.layout.Box
import androidx.compose.ui.Modifier
import mycelium.ui.verticalScrollWithBar
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.InstructionsSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val NodesJson = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[],"logins":[],"instructions_revision":"afad7763c4d5e6f708192a3b4c5d6e7f80910203","instructions_unmanaged":["Claude Code: /home/sam/.claude/CLAUDE.md"],"sessions_on_older_instructions":2},
{"id":"n2","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","online":false,"draining":false,
"agents":[],"logins":[],"instructions_revision":"85d49d3aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]"""

private const val DocumentJson = """{"source":{"forge_id":"f1","repository":"slop/agents-md","path":"AGENTS.md"},"revision":"afad7763c4d5e6f708192a3b4c5d6e7f80910203","layers":[
{"layer":"global","name":"Global","path":"AGENTS.md","text":"## Diffs","version":{"id":"afad7763c4d5e6f708192a3b4c5d6e7f80910203","created_at":"2026-10-22T10:00:00Z"}},
{"layer":"g1","name":"slop","path":"AGENTS.slop.md","text":"Use British English.","version":null},
{"layer":"g2","name":"work","path":"AGENTS.work.md","text":"","version":null}]}"""

private const val GroupsJson = """[{"id":"g1","name":"slop","folders":[{"id":"f1","node_id":null,"prefix":"/home/claude/projects"},{"id":"f2","node_id":"n1","prefix":"/srv/slop"}],"created_at":"2026-10-22T10:00:00Z"},
{"id":"g2","name":"work","folders":[],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val ExtensionsJson = """{"generated_ui":true,"rich_markdown":true,"mycelium_tools":true}"""

private const val ProjectsJson = """[{"id":"p1","node_id":"n1","name":"mycelium","path":"/p/mycelium","group_id":null,"created_at":"2026-10-22T10:00:00Z"}]"""

class InstructionsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<Triple<HttpMethod, String, String>>())

    private fun requestTo(method: HttpMethod, pathEnd: String): Triple<HttpMethod, String, String> {
        rule.waitUntil(timeoutMillis = 5_000) { requests.toList().any { it.first == method && it.second.endsWith(pathEnd) } || false }

        return requests.toList().first { it.first == method && it.second.endsWith(pathEnd) }
    }

    private fun show(document: String = DocumentJson) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += Triple(request.method, path, request.body.toByteArray().decodeToString())
            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/conversations") -> "[]"
                path.endsWith("/nodes") -> NodesJson
                path.endsWith("/instructions") -> document
                path.endsWith("/extensions") -> ExtensionsJson
                path.endsWith("/instruction-proposals") -> "[]"
                path.endsWith("/groups") && request.method == HttpMethod.Get -> GroupsJson
                path.endsWith("/projects") -> ProjectsJson
                path.endsWith("/forges") -> "[]"
                path.contains("/projects/") -> """{"id":"p1","node_id":"n1","name":"mycelium","path":"/p/mycelium","group_id":"g1","created_at":"2026-10-22T10:00:00Z"}"""
                path.contains("/instructions/") -> """{"layer":"global","name":"Global","text":"x"}"""
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Box(Modifier.verticalScrollWithBar()) { InstructionsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("agents-md / AGENTS.md").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theInstructionsPageShowsOnlyTheGlobalLayer() {
        show()

        rule.onNodeWithText("agents-md / AGENTS.md").assertExists()
        rule.onNodeWithText("afad776 · ", substring = true).assertExists()
        rule.onNodeWithText("## Diffs").assertExists()
        assertEquals(0, rule.onAllNodesWithContentDescription("Instructions for the slop group").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("New group").fetchSemanticsNodes().size)
    }

    @Test
    fun anUnmanagedFileCanBeHandedToMyceliumWhichKeepsTheOriginalAsABackup() {
        show()

        rule.onNodeWithText("Your file is kept as CLAUDE.md.bak.").assertExists()
        rule.onNodeWithContentDescription("More for instructions on willow").performScrollTo().performClick()
        rule.onNodeWithText("Manage CLAUDE.md").performClick()

        requestTo(HttpMethod.Post, "/nodes/n1/instructions/take-over")
    }

    @Test
    fun eachNodeShowsTheVersionItHoldsAndItsSessionsOnAnOlderVersion() {
        show()

        rule.onNodeWithText("afad776").assertExists()
        rule.onNodeWithText("2, update on their next message").assertExists()
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

        rule.onNodeWithText("Save and commit").performScrollTo().performClick()

        val saved = requestTo(HttpMethod.Put, "/instructions/global")
        assertEquals("""{"text":"## Diffs"}""", saved.third)
    }

    @Test
    fun theExtensionsAreOnAndEachSwitchSavesAllThreeWithItselfTurnedOff() {
        show()

        rule.onNodeWithText("Mycelium extensions").assertExists()
        rule.onNodeWithContentDescription("Generated UI").assertExists()
        rule.onNodeWithContentDescription("Rich Markdown").performScrollTo().performClick()

        assertEquals("""{"generated_ui":true,"rich_markdown":false,"mycelium_tools":true}""", requestTo(HttpMethod.Put, "/extensions").third)
    }

    @Test
    fun theHintsSayWhatEachExtensionAddsAndWhichAgentsGetIt() {
        show()

        rule.onNodeWithText("Teaches agents to answer", substring = true).assertExists()
        rule.onNodeWithText("Added to Claude Code and Codex sessions and to chats.", substring = true).assertExists()
        rule.onNodeWithText("recall, remember, walkthrough", substring = true).assertExists()
    }

    @Test
    fun theEditorIsCappedAndPrecedesNodeSynchronisation() {
        val longText = (1..300).joinToString("\\n") { "line $it" }
        show(DocumentJson.replace("## Diffs", longText).replace("Use British English.", longText))

        rule.waitForIdle()

        val editor = rule.onNodeWithContentDescription("Edit Global instructions").onParent().getUnclippedBoundsInRoot()

        val editorHeight = editor.bottom - editor.top

        assertTrue(editorHeight in 320.dp..800.dp, "editor is $editorHeight")

        for (after in listOf(rule.onNodeWithText("Mycelium extensions"), rule.onNodeWithText("Node synchronisation"))) {
            assertTrue(after.getUnclippedBoundsInRoot().top >= editor.bottom, "operational controls precede the editor")
        }

        assertTrue(rule.onNodeWithText("Save and commit").getUnclippedBoundsInRoot().top >= editor.bottom)
    }
}
