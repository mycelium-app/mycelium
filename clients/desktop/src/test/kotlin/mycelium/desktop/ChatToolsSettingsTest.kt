package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
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

    private val requests = mutableListOf<HttpRequestData>()
    private val bodies = mutableListOf<String>()
    private var postAnswer: Pair<HttpStatusCode, String> = HttpStatusCode.Created to GitHubJson

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += request
            bodies += String(request.body.toByteArray())

            when {
                request.method == HttpMethod.Post -> respond(postAnswer.second, postAnswer.first, json)
                request.method == HttpMethod.Patch -> respond(LabJson.replace("\"enabled\":true", "\"enabled\":false"), HttpStatusCode.OK, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                else -> respond("[$LabJson]", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    @Test
    fun theListShowsEachServerWithItsMaskedTokenAndItsTools() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Lab").assertExists()
        rule.onNodeWithText("Ends in ••••cdef").assertExists()
        rule.onNodeWithText("get_weather, list_files").assertExists()
        rule.onNodeWithContentDescription("Use Lab in chats").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "On"))
        rule.onNodeWithContentDescription("Always allow tools from Lab").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Off"))
    }

    @Test
    fun addingAServerSendsItsAddressAndAMaskedTokenAndNeverShowsTheToken() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add a server").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("GitHub")
        rule.onNodeWithContentDescription("Streamable HTTP address").performTextInput("https://gh.test/mcp")
        rule.onNodeWithContentDescription("Bearer token, if the server needs one").performTextInput("ghp-secret")
        rule.onNodeWithContentDescription("Bearer token, if the server needs one").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Add server").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("GitHub").fetchSemanticsNodes().isNotEmpty() && rule.onAllNodesWithText("Add a server").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"name":"GitHub","url":"https://gh.test/mcp","bearer_token":"ghp-secret"}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("ghp-secret").assertCountEquals(0)
        rule.onNodeWithText("No token").assertExists()
    }

    @Test
    fun aRefusedServerShowsWhatToFixAndKeepsTheForm() {
        postAnswer = HttpStatusCode.BadRequest to """{"error":"tool_server_failed","message":"GitHub did not accept this token. Check that it has not expired and is for this server, then try again."}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add a server").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Add server").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Add server").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("GitHub did not accept this token. Check that it has not expired and is for this server, then try again.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Name").assertExists()
    }

    @Test
    fun switchingAServerOffAndRemovingItAreSentAndShown() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithContentDescription("Use Lab in chats").performClick()
        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Patch } }
        assertEquals("""{"enabled":false}""", bodies.last { it.isNotEmpty() })
        rule.waitUntil(5_000) { runCatching { rule.onNodeWithContentDescription("Use Lab in chats").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Off")) }.isSuccess }

        rule.onNodeWithContentDescription("Remove Lab").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Lab").fetchSemanticsNodes().isEmpty() }
    }
}
