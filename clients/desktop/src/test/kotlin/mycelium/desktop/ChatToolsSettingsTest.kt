package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ChatToolsSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val LabJson = """{"id":"s1","name":"Lab","url":"https://lab.test/mcp","token_hint":"••••cdef","enabled":true,"always_allow":false,"tools":[{"name":"get_weather","description":"Get the weather"},{"name":"list_files","description":"List files"}],"created_at":"2026-10-22T10:00:00Z"}"""

private const val GitHubJson = """{"id":"s2","name":"GitHub","url":"https://gh.test/mcp","enabled":true,"always_allow":false,"tools":[],"created_at":"2026-10-22T11:00:00Z"}"""

class ChatToolsSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<HttpRequestData>()
    private val bodies = CopyOnWriteArrayList<String>()
    private var listed = LabJson
    private var patchAnswer: Pair<HttpStatusCode, String>? = null
    private var postAnswer: Pair<HttpStatusCode, String> = HttpStatusCode.Created to GitHubJson

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += request
            bodies += String(request.body.toByteArray())

            when {
                request.method == HttpMethod.Post -> respond(postAnswer.second, postAnswer.first, json)
                request.method == HttpMethod.Patch -> patchAnswer?.let { respond(it.second, it.first, json) } ?: respond(LabJson.replace("\"enabled\":true", "\"enabled\":false"), HttpStatusCode.OK, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                else -> respond("[$listed]", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    @Test
    fun theListShowsEachServerWithItsMaskedTokenAndItsTools() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Integrations").assertExists()
        rule.onNodeWithText("Lab").assertExists()
        rule.onNodeWithText("MCP").assertExists()
        rule.onNodeWithText("Ends in ••••cdef").assertExists()
        rule.onNodeWithText("2 tools").assertExists()
        rule.onNodeWithContentDescription("Use Lab").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "On"))
        rule.onNodeWithContentDescription("Always allow tools from Lab").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Off"))
    }

    @Test
    fun addingAServerSendsItsAddressAndAMaskedTokenAndNeverShowsTheToken() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("GitHub")
        rule.onNodeWithContentDescription("Streamable HTTP address").performTextInput("https://gh.test/mcp")
        rule.onNodeWithContentDescription("Bearer token, if the server needs one").performTextInput("ghp-secret")
        rule.onNodeWithContentDescription("Bearer token, if the server needs one").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Connect integration").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("ghp-secret").fetchSemanticsNodes().isEmpty() && rule.onAllNodesWithText("No token").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"name":"GitHub","url":"https://gh.test/mcp","bearer_token":"ghp-secret"}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("ghp-secret").assertCountEquals(0)
        rule.onNodeWithText("No token").assertExists()
    }

    @Test
    fun aGatewayEndpointCanBeSubmittedFromTheBearerTokenFieldWithEnter() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Gateway")
        rule.onNodeWithContentDescription("Streamable HTTP address").performTextInput("https://gateway.test/mcp")
        rule.onNodeWithContentDescription("Bearer token, if the server needs one").performTextInput("test-gateway-key")
        rule.onNodeWithContentDescription("Bearer token, if the server needs one").performKeyInput { pressKey(Key.Enter) }

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Post } }
        assertEquals("""{"name":"Gateway","url":"https://gateway.test/mcp","bearer_token":"test-gateway-key"}""", bodies.last { it.isNotEmpty() })
        rule.waitUntil(5_000) { rule.onAllNodesWithText("test-gateway-key").fetchSemanticsNodes().isEmpty() }
        rule.onAllNodesWithText("test-gateway-key").assertCountEquals(0)
    }

    @Test
    fun hundredsOfToolsStayInACompactSummaryAndRefreshFromTheRowMenu() {
        val tools = (1..500).joinToString(",") { """{"name":"tool_$it","description":"Tool $it"}""" }
        listed = LabJson.replace("""{"name":"get_weather","description":"Get the weather"},{"name":"list_files","description":"List files"}""", tools)
        postAnswer = HttpStatusCode.OK to LabJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("500 tools").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("tool_1").assertDoesNotExist()
        rule.onNodeWithContentDescription("More for Lab").performClick()
        rule.onNodeWithText("Refresh tools").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("2 tools").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("/api/v1/chat-tool-servers/s1/refresh", requests.last { it.method == HttpMethod.Post }.url.encodedPath)
    }

    @Test
    fun aRefusedServerShowsWhatToFixAndKeepsTheForm() {
        postAnswer = HttpStatusCode.BadRequest to """{"error":"tool_server_failed","message":"GitHub did not accept this token. Check that it has not expired and is for this server, then try again."}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connect integration").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Connect integration").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("GitHub did not accept this token. Check that it has not expired and is for this server, then try again.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Name").assertExists()
    }

    @Test
    fun switchingAServerOffAndRemovingItAreSentAndShown() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithContentDescription("Use Lab").performClick()
        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Patch } }
        assertEquals("""{"enabled":false}""", bodies.last { it.isNotEmpty() })
        rule.waitUntil(5_000) { runCatching { rule.onNodeWithContentDescription("Use Lab").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Off")) }.isSuccess }

        rule.onNodeWithContentDescription("More for Lab").performClick()
        rule.onNodeWithText("Remove integration").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Lab").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun renamingAnIntegrationFromTheRowMenuSendsTheTrimmedNameAndShowsIt() {
        patchAnswer = HttpStatusCode.OK to LabJson.replace("\"name\":\"Lab\"", "\"name\":\"Weather\"")
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithContentDescription("More for Lab").performClick()
        rule.onNodeWithText("Rename").performClick()
        rule.onNodeWithContentDescription("Integration name").performTextReplacement("  Weather  ")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Rename integration").fetchSemanticsNodes().isEmpty() }
        assertEquals("""{"name":"Weather"}""", bodies.last { it.isNotEmpty() })
        assertEquals("/api/v1/chat-tool-servers/s1", requests.last { it.method == HttpMethod.Patch }.url.encodedPath)
        rule.onNodeWithText("Weather").assertExists()
    }

    @Test
    fun aRefusedRenameShowsTheServersMessageAndKeepsTheDialog() {
        patchAnswer = HttpStatusCode.Conflict to """{"error":"name_taken","message":"Enter a different name. Another tool server already has this one."}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithContentDescription("More for Lab").performClick()
        rule.onNodeWithText("Rename").performClick()
        rule.onNodeWithContentDescription("Integration name").performTextReplacement("GitHub")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Enter a different name. Another tool server already has this one.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Rename integration").assertExists()
    }
}
