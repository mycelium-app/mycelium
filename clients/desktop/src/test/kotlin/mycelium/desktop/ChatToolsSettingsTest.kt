package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.width
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollTo
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
import androidx.compose.runtime.CompositionLocalProvider
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.api.StreamMessage
import mycelium.ui.LocalPlatform
import mycelium.state.Store
import mycelium.ui.ChatToolsSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val LabJson = """{"id":"s1","name":"Lab","url":"https://lab.test/mcp","token_hint":"••••cdef","enabled":true,"always_allow":false,"tools":[{"name":"get_weather","description":"Get the weather"},{"name":"list_files","description":"List files"}],"created_at":"2026-10-22T10:00:00Z"}"""

private const val ZulipJson = """{"id":"s3","name":"Zulip","kind":"zulip","url":"https://chat.test","account":"ada@example.com","token_hint":"••••cdef","enabled":true,"always_allow":false,"tools":[{"name":"list_channels","description":"List channels"},{"name":"send_message","description":"Send a message"}],"created_at":"2026-10-22T12:00:00Z"}"""

private const val EmailJson = """{"id":"s4","name":"Email","kind":"email","url":"","account":"ada@example.com","settings":{"imap_host":"imap.example.com","imap_port":993,"imap_tls":"tls","smtp_host":"smtp.example.com","smtp_port":587,"smtp_tls":"starttls"},"token_hint":"••••0123","enabled":true,"always_allow":false,"tools":[{"name":"list_folders","description":"List folders"},{"name":"send_message","description":"Send"}],"created_at":"2026-10-22T13:00:00Z"}"""

private const val StoatJson = """{"id":"s5","name":"Stoat","kind":"stoat","url":"https://api.stoat.chat","settings":{"token_kind":"bot"},"token_hint":"••••cdef","enabled":true,"always_allow":false,"tools":[{"name":"list_servers","description":"List servers"}],"created_at":"2026-10-22T14:00:00Z"}"""

private const val LastFmJson = """{"id":"s8","name":"Last.fm","kind":"lastfm","url":"https://ws.audioscrobbler.com/2.0/","account":"ada","settings":{"api_key":"key-1"},"token_hint":"••••5678","enabled":true,"always_allow":false,"tools":[{"name":"get_recent_tracks","description":"Recent"}],"created_at":"2026-10-22T16:00:00Z"}"""

private const val GitHubJson = """{"id":"s2","name":"GitHub","url":"https://gh.test/mcp","enabled":true,"always_allow":false,"tools":[],"created_at":"2026-10-22T11:00:00Z"}"""

@Composable
private fun Scrolling(content: @Composable () -> Unit) {
    Column(Modifier.verticalScroll(rememberScrollState())) { content() }
}

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
    fun theHomeShowsEachIntegrationAsACardAndItsPageShowsTheMaskedToken() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Scrolling { ChatToolsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Lab").fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText("Connected").assertCountEquals(2)
        rule.onNodeWithText("2 tools").assertExists()
        rule.onNodeWithText("Lab").performClick()

        rule.onNodeWithText("MCP server · Developer").assertExists()
        rule.onNodeWithContentDescription("Open lab.test/mcp").performClick()
        rule.onAllNodesWithText("Ends in ••••cdef")[0].assertExists()
    }

    @Test
    fun aLiteLlmGatewayCanBeRefreshedFromItsAccountMenu() {
        listed = """{"id":"s7","name":"Gateway","kind":"litellm","url":"https://litellm.test/mcp/","token_hint":"••••cdef","enabled":true,"always_allow":false,"tools":[{"name":"a","description":"A"}],"created_at":"2026-10-22T15:00:00Z"}"""
        postAnswer = HttpStatusCode.OK to listed
        val store = store()
        rule.setContent { MyceliumTheme(true) { Scrolling { ChatToolsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("LiteLLM").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("LiteLLM").performClick()
        rule.onNodeWithContentDescription("Open litellm.test/mcp").performClick()
        rule.onNodeWithText("Refresh tools").performScrollTo().performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/s7/refresh") } }
    }

    @Test
    fun hundredsOfToolsStayInACompactSummaryAndRefreshFromTheAccountMenu() {
        val tools = (1..500).joinToString(",") { """{"name":"tool_$it","description":"Tool $it"}""" }
        listed = LabJson.replace("""{"name":"get_weather","description":"Get the weather"},{"name":"list_files","description":"List files"}""", tools)
        postAnswer = HttpStatusCode.OK to LabJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { Scrolling { ChatToolsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("500 tools").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("tool_1").assertDoesNotExist()
        rule.onNodeWithText("Lab").performClick()
        rule.onNodeWithContentDescription("Open lab.test/mcp").performClick()
        rule.onNodeWithText("Refresh tools").performScrollTo().performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Post && it.url.encodedPath == "/api/v1/chat-tool-servers/s1/refresh" } }
    }

    @Test
    fun disablingAnIntegrationAndRemovingItAreSentAndShown() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Scrolling { ChatToolsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Lab").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Lab").performClick()
        rule.onNodeWithText("Disable").performScrollTo().performClick()
        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Patch } }
        assertEquals("""{"enabled":false}""", bodies.last { it.isNotEmpty() })
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Enable").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Remove Lab").performScrollTo().performClick()
        rule.onNodeWithText("Remove").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("No integrations yet").fetchSemanticsNodes().isNotEmpty() }
        assertTrue(requests.any { it.method == HttpMethod.Delete })
    }

    @Test
    fun renamingAnAccountSendsTheTrimmedNameAndShowsIt() {
        patchAnswer = HttpStatusCode.OK to LabJson.replace("\"name\":\"Lab\"", "\"name\":\"Weather\"")
        val store = store()
        rule.setContent { MyceliumTheme(true) { Scrolling { ChatToolsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Lab").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Lab").performClick()
        rule.onNodeWithContentDescription("Open lab.test/mcp").performClick()
        rule.onNodeWithText("Rename account").performScrollTo().performClick()
        rule.onNodeWithContentDescription("Account name").performTextReplacement("  Weather  ")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Account name").fetchSemanticsNodes().isEmpty() }
        assertEquals("""{"name":"Weather"}""", bodies.last { it.isNotEmpty() })
        assertEquals("/api/v1/chat-tool-servers/s1", requests.last { it.method == HttpMethod.Patch }.url.encodedPath)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Weather").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aRefusedRenameShowsTheServersMessageAndKeepsTheDialog() {
        patchAnswer = HttpStatusCode.Conflict to """{"error":"name_taken","message":"Enter a different name. Another tool server already has this one."}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { Scrolling { ChatToolsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Lab").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Lab").performClick()
        rule.onNodeWithContentDescription("Open lab.test/mcp").performClick()
        rule.onNodeWithText("Rename account").performScrollTo().performClick()
        rule.onNodeWithContentDescription("Account name").performTextReplacement("GitHub")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Enter a different name. Another tool server already has this one.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Account name").assertExists()
    }

    @Test
    fun aBuiltInIntegrationHasNoRefreshToolsMenuItem() {
        listed = ZulipJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { Scrolling { ChatToolsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Zulip").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Zulip").performClick()
        rule.onNodeWithContentDescription("Open ada@example.com").performClick()
        rule.onNodeWithText("Rename account").assertExists()
        rule.onNodeWithText("Refresh tools").assertDoesNotExist()
    }

    @Test
    fun signInToLastFmOpensTheAuthoriseUrlAndTheRowShowsItWhenSignedIn() {
        val opened = CopyOnWriteArrayList<String>()
        val platform = object : Platform {
            override val originUrl: String? = null
            override val signsInThroughTheBrowser = false
            override fun loadConnection(): Connection? = null
            override fun saveConnection(connection: Connection?) = Unit
            override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
            override fun approveCode(): String? = null
            override fun currentPath() = "/"
            override fun navigate(url: String) = Unit
            override fun openUrl(url: String) { opened += url }
            override fun copyToClipboard(text: String) = Unit
        }
        listed = LastFmJson
        postAnswer = HttpStatusCode.OK to """{"authorize_url":"https://www.last.fm/api/auth/?api_key=key-1"}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { CompositionLocalProvider(LocalPlatform provides platform) { Scrolling { ChatToolsSettings(store) } } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Sign in to love tracks and scrobble").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Last.fm").performClick()
        rule.onNodeWithContentDescription("Open ada").performClick()
        rule.onNodeWithText("Re-authenticate").performScrollTo().performClick()

        rule.waitUntil(5_000) { opened.isNotEmpty() }
        assertEquals(listOf("https://www.last.fm/api/auth/?api_key=key-1"), opened.toList())
        assertTrue(requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/s8/lastfm-sign-in") })

        listed = LastFmJson.replace("\"enabled\":true", "\"signed_in\":true,\"enabled\":true")
        store.receive(StreamMessage.IntegrationsChanged)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Signed in to Last.fm").fetchSemanticsNodes().isNotEmpty() }
    }
}
