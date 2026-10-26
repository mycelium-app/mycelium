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
    fun outlineAsksForItsAddressAndApiKeyAndIsListedAsOutline() {
        postAnswer = HttpStatusCode.Created to """{"id":"s6","name":"Docs","url":"https://docs.test/mcp","settings":{"app":"outline"},"token_hint":"••••cdef","enabled":true,"always_allow":false,"tools":[{"name":"search","description":"Search"}],"created_at":"2026-10-22T15:00:00Z"}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("Outline").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Docs")
        rule.onNodeWithContentDescription("Outline address").performTextInput("https://docs.test")
        rule.onNodeWithContentDescription("Outline API key").performTextInput("ol_api_secret")
        rule.onNodeWithContentDescription("Outline API key").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Connect integration").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connect integration").fetchSemanticsNodes().isEmpty() }
        assertEquals("""{"name":"Docs","url":"https://docs.test","bearer_token":"ol_api_secret","kind":"outline"}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("ol_api_secret").assertCountEquals(0)
        rule.onNodeWithText("Docs").assertExists()
        rule.onNodeWithText("Outline").assertExists()
    }

    @Test
    fun addingALiteLlmGatewaySendsItsKindAddressAndMaskedKey() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("LiteLLM").performClick()
        rule.onNodeWithContentDescription("LiteLLM address").performTextInput("https://litellm.test")
        rule.onNodeWithContentDescription("LiteLLM key").performTextInput("sk-secret-1")
        rule.onNodeWithContentDescription("LiteLLM key").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Connect integration").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connect integration").fetchSemanticsNodes().isEmpty() }
        assertEquals("""{"name":"LiteLLM","url":"https://litellm.test","bearer_token":"sk-secret-1","kind":"litellm"}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("sk-secret-1").assertCountEquals(0)
    }

    @Test
    fun aLiteLlmGatewayRowShowsItsTypeAndCanBeRefreshed() {
        listed = """{"id":"s7","name":"Gateway","kind":"litellm","url":"https://litellm.test/mcp/","token_hint":"••••cdef","enabled":true,"always_allow":false,"tools":[{"name":"a","description":"A"}],"created_at":"2026-10-22T15:00:00Z"}"""
        postAnswer = HttpStatusCode.OK to listed
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Gateway").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("LiteLLM").assertExists()
        rule.onNodeWithContentDescription("More for Gateway").performClick()
        rule.onNodeWithText("Refresh tools").performClick()

        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/s7/refresh") } }
    }

    @Test
    fun signInWithOutlineOpensTheBrowserAndTheIntegrationAppearsWhenTheSignInFinishes() {
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
        postAnswer = HttpStatusCode.OK to """{"authorize_url":"https://docs.test/authorize?state=s"}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { CompositionLocalProvider(LocalPlatform provides platform) { ChatToolsSettings(store) } } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("Outline").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Docs")
        rule.onNodeWithContentDescription("Outline address").performTextInput("https://docs.test")
        rule.onNodeWithText("Sign in with Outline").performClick()

        rule.waitUntil(5_000) { opened.isNotEmpty() }
        assertEquals(listOf("https://docs.test/authorize?state=s"), opened.toList())
        assertEquals("""{"name":"Docs","url":"https://docs.test","kind":"outline"}""", bodies.last { it.isNotEmpty() })
        rule.onNodeWithText("Finish signing in in your browser.").assertExists()

        listed = """{"id":"s6","name":"Docs","url":"https://docs.test/mcp","settings":{"app":"outline"},"oauth":true,"enabled":true,"always_allow":false,"tools":[{"name":"search","description":"Search"}],"created_at":"2026-10-22T15:00:00Z"}"""
        store.receive(StreamMessage.IntegrationsChanged)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Signed in with OAuth").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Finish signing in in your browser.").assertDoesNotExist()
        rule.onNodeWithText("Connect integration").assertDoesNotExist()
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

    @Test
    fun choosingZulipAsksForTheServerEmailAndApiKeyAndShowsItAsBuiltIn() {
        postAnswer = HttpStatusCode.Created to ZulipJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("Zulip").performClick()
        rule.onNodeWithContentDescription("Zulip server address").performTextInput("https://chat.test")
        rule.onNodeWithContentDescription("Zulip email address").performTextInput("ada@example.com")
        rule.onNodeWithContentDescription("Zulip API key").performTextInput("zulip-secret")
        rule.onNodeWithContentDescription("Zulip API key").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Connect integration").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Built-in").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"name":"Zulip","url":"https://chat.test","bearer_token":"zulip-secret","kind":"zulip","account":"ada@example.com"}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("zulip-secret").assertCountEquals(0)
        rule.onNodeWithText("ada@example.com").assertExists()
    }

    @Test
    fun aBuiltInIntegrationHasNoRefreshToolsMenuItem() {
        listed = ZulipJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Built-in").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for Zulip").performClick()
        rule.onNodeWithText("Rename").assertExists()
        rule.onNodeWithText("Refresh tools").assertDoesNotExist()
    }

    @Test
    fun choosingEmailAsksForTheMailServersAndSendsTheirSettingsWithoutShowingThePassword() {
        postAnswer = HttpStatusCode.Created to EmailJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("Email").performClick()
        rule.onNodeWithContentDescription("Email username").performTextInput("ada@example.com")
        rule.onNodeWithContentDescription("Email password or app password").performTextInput("mail-secret")
        rule.onNodeWithContentDescription("Email password or app password").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithContentDescription("IMAP server").performTextInput("imap.example.com")
        rule.onNodeWithContentDescription("SMTP server").performTextInput("smtp.example.com")
        rule.onNodeWithContentDescription("SMTP port, if not the usual one").performTextInput("587")
        rule.onAllNodesWithText("STARTTLS")[1].performClick()
        rule.onNodeWithText("Connect integration").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Built-in").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(
            """{"name":"Email","url":"","bearer_token":"mail-secret","kind":"email","account":"ada@example.com","settings":{"imap_host":"imap.example.com","imap_tls":"tls","smtp_host":"smtp.example.com","smtp_tls":"starttls","smtp_port":587}}""",
            bodies.last { it.isNotEmpty() },
        )
        rule.onAllNodesWithText("mail-secret").assertCountEquals(0)
        rule.onNodeWithText("ada@example.com").assertExists()
    }

    @Test
    fun anEmailPortThatIsNotANumberIsExplainedAndNothingIsSent() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("Email").performClick()
        rule.onNodeWithContentDescription("IMAP port, if not the usual one").performTextInput("imap")
        rule.onNodeWithText("Connect integration").performClick()

        rule.onNodeWithText("Enter the IMAP port as a number, for example 993.").assertExists()
        assertEquals(0, requests.count { it.method == HttpMethod.Post })
    }

    @Test
    fun choosingStoatAsksForTheServerAddressAndTokenAndSendsTheTokenType() {
        postAnswer = HttpStatusCode.Created to StoatJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("Stoat").performClick()
        rule.onNodeWithContentDescription("Stoat API address").performTextInput("https://api.stoat.chat")
        rule.onNodeWithText("Bot").performClick()
        rule.onNodeWithContentDescription("Stoat token").performTextInput("stoat-secret")
        rule.onNodeWithContentDescription("Stoat token").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Connect integration").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Built-in").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"name":"Stoat","url":"https://api.stoat.chat","bearer_token":"stoat-secret","kind":"stoat","settings":{"token_kind":"bot"}}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("stoat-secret").assertCountEquals(0)
    }

    @Test
    fun stoatUsesItsOwnAddressWhenNoneIsGiven() {
        postAnswer = HttpStatusCode.Created to StoatJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("Stoat").performClick()
        rule.onNodeWithContentDescription("Stoat token").performTextInput("stoat-secret")
        rule.onNodeWithText("Connect integration").performClick()

        rule.waitUntil(5_000) { bodies.any { it.contains("stoat-secret") } }
        assertTrue(bodies.last { it.isNotEmpty() }.contains("\"url\":\"https://stoat.chat/api\""), bodies.last())
    }

    @Test
    fun lastFmNeedsAKeyASecretAndAUsernameAndSendsThemWithoutAnAddress() {
        postAnswer = HttpStatusCode.Created to LastFmJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { ChatToolsSettings(store) } }

        rule.onNodeWithText("Add integration").performClick()
        rule.onNodeWithText("Last.fm").performClick()
        rule.onNodeWithContentDescription("Last.fm username").performTextInput("ada")
        rule.onNodeWithContentDescription("Last.fm API key").performTextInput("key-1")
        rule.onNodeWithContentDescription("Last.fm shared secret").performTextInput("secret-5678")
        rule.onNodeWithContentDescription("Last.fm shared secret").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Connect integration").performClick()

        rule.waitUntil(5_000) { bodies.any { it.contains("secret-5678") } }
        assertEquals("""{"name":"Last.fm","url":"","bearer_token":"secret-5678","kind":"lastfm","account":"ada","settings":{"api_key":"key-1"}}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("secret-5678").assertCountEquals(0)
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
        rule.setContent { MyceliumTheme(true) { CompositionLocalProvider(LocalPlatform provides platform) { ChatToolsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Not signed in: needed to love tracks and scrobble").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for Last.fm").performClick()
        rule.onNodeWithText("Sign in to Last.fm").performClick()

        rule.waitUntil(5_000) { opened.isNotEmpty() }
        assertEquals(listOf("https://www.last.fm/api/auth/?api_key=key-1"), opened.toList())
        assertTrue(requests.any { it.method == HttpMethod.Post && it.url.encodedPath.endsWith("/s8/lastfm-sign-in") })

        listed = LastFmJson.replace("\"enabled\":true", "\"signed_in\":true,\"enabled\":true")
        store.receive(StreamMessage.IntegrationsChanged)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Signed in to Last.fm").fetchSemanticsNodes().isNotEmpty() }
    }
}
