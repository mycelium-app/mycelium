package mycelium.desktop

import androidx.compose.ui.test.assertCountEquals
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
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.SetupFlow
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Created = """{"id":"s1","name":"example.com","url":"https://example.com/mcp","token_hint":"••••cdef","enabled":true,"tools":[],"created_at":"2026-10-27T10:00:00Z"}"""

class SetupFlowTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Pair<String, String>>()
    private var testAnswer = HttpStatusCode.OK to """{"tools":[{"name":"search","kind":"read"},{"name":"create_doc","kind":"write"}]}"""
    private var finished = false

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += path to String(request.body.toByteArray())

            when {
                path.endsWith("/probe") -> respond("""{"oauth":false}""", HttpStatusCode.OK, json)
                path.endsWith("/test") -> respond(testAnswer.second, testAnswer.first, json)
                else -> respond(Created, HttpStatusCode.Created, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    private fun open(store: Store, taken: Set<String> = emptySet(), kind: String = "mcp") {
        rule.setContent { MyceliumTheme(true) { SetupFlow(store, kind, null, taken, {}, { finished = true }, {}, {}) } }
    }

    private fun toTestStep() {
        rule.onNodeWithContentDescription("Server address").performTextInput("https://example.com/mcp")
        rule.onNodeWithText("Continue").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Token").fetchSemanticsNodes().isNotEmpty() || rule.onAllNodesWithContentDescription("Token").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Token").performTextInput("secret-token")
        rule.onNodeWithText("Continue").performClick()
    }

    @Test
    fun theStripNamesTheStepAndTheTestRunsByItselfOnArrival() {
        val store = store()
        open(store)

        rule.onNodeWithContentDescription("Step 1 of 4: Address").assertExists()
        toTestStep()

        rule.onNodeWithContentDescription("Step 3 of 4: Test").assertExists()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connected. The server offers 2 tools.").fetchSemanticsNodes().isNotEmpty() }
        assertTrue(requests.any { it.first.endsWith("/test") && it.second.contains("secret-token") })
        assertTrue(requests.none { it.first.endsWith("/chat-tool-servers") })
    }

    @Test
    fun connectingSendsTheTokenAndNoExpiryByDefaultThenFinishes() {
        val store = store()
        open(store)

        toTestStep()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connected. The server offers 2 tools.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { finished }
        assertEquals("""{"name":"example.com","url":"https://example.com/mcp","bearer_token":"secret-token"}""", requests.last { it.first.endsWith("/chat-tool-servers") }.second)
    }

    @Test
    fun aFailedTestStaysOnTheTestStepWithBothWaysOut() {
        testAnswer = HttpStatusCode.BadRequest to """{"error":"tool_server_failed","message":"The server refused that token."}"""
        val store = store()
        open(store)

        toTestStep()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("The server refused that token.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Step 3 of 4: Test").assertExists()
        rule.onNodeWithText("Back to address").assertExists()
        rule.onNodeWithText("Try again").assertExists()
    }

    @Test
    fun backKeepsWhatWasTypedAndANameAnotherIntegrationUsesIsRefused() {
        val store = store()
        open(store, setOf("example_com"))

        toTestStep()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connected. The server offers 2 tools.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Back").performClick()
        rule.onNodeWithText("Back").performClick()
        rule.onAllNodesWithText("https://example.com/mcp").assertCountEquals(1)
        rule.onNodeWithText("Continue").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Token").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Continue").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connected. The server offers 2 tools.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()

        rule.onNodeWithText("Enter a different name. Another integration already has this one.", substring = true).assertExists()
    }

    @Test
    fun zulipRunsThroughAddressCredentialAndNameAndSendsItsAccount() {
        val store = store()
        open(store, kind = "zulip")

        rule.onNodeWithContentDescription("Step 1 of 3: Address").assertExists()
        rule.onNodeWithContentDescription("Zulip server address").performTextInput("https://chat.test")
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithContentDescription("Zulip email address").performTextInput("ada@example.com")
        rule.onNodeWithContentDescription("Zulip API key").performTextInput("zulip-secret")
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { finished }
        assertEquals("""{"name":"Zulip","url":"https://chat.test","bearer_token":"zulip-secret","kind":"zulip","account":"ada@example.com"}""", requests.last { it.first.endsWith("/chat-tool-servers") }.second)
        rule.onAllNodesWithText("zulip-secret").assertCountEquals(0)
    }

    @Test
    fun emailSendsItsMailServersAndSecurityWithoutShowingThePassword() {
        val store = store()
        open(store, kind = "email")

        rule.onNodeWithContentDescription("IMAP server").performTextInput("imap.example.com")
        rule.onNodeWithContentDescription("SMTP server").performTextInput("smtp.example.com")
        rule.onNodeWithContentDescription("SMTP port, if not the usual one").performTextInput("587")
        rule.onAllNodesWithText("STARTTLS")[1].performClick()
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithContentDescription("Email username").performTextInput("ada@example.com")
        rule.onNodeWithContentDescription("Email password or app password").performTextInput("mail-secret")
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { finished }
        assertEquals(
            """{"name":"Email","url":"","bearer_token":"mail-secret","kind":"email","account":"ada@example.com","settings":{"imap_host":"imap.example.com","imap_tls":"tls","smtp_host":"smtp.example.com","smtp_tls":"starttls","smtp_port":587}}""",
            requests.last { it.first.endsWith("/chat-tool-servers") }.second,
        )
        rule.onAllNodesWithText("mail-secret").assertCountEquals(0)
    }

    @Test
    fun anEmailPortThatIsNotANumberIsExplainedAndTheStepDoesNotAdvance() {
        val store = store()
        open(store, kind = "email")

        rule.onNodeWithContentDescription("IMAP server").performTextInput("imap.example.com")
        rule.onNodeWithContentDescription("SMTP server").performTextInput("smtp.example.com")
        rule.onNodeWithContentDescription("IMAP port, if not the usual one").performTextInput("imap")
        rule.onNodeWithText("Continue").performClick()

        rule.onNodeWithText("Enter the IMAP port as a number, for example 993.").assertExists()
        rule.onNodeWithContentDescription("Step 1 of 3: Address").assertExists()
    }

    @Test
    fun stoatUsesItsOwnAddressWhenNoneIsGivenAndSendsTheTokenType() {
        val store = store()
        open(store, kind = "stoat")

        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Bot").performClick()
        rule.onNodeWithContentDescription("Stoat token").performTextInput("stoat-secret")
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { finished }
        assertEquals("""{"name":"Stoat","url":"https://stoat.chat/api","bearer_token":"stoat-secret","kind":"stoat","settings":{"token_kind":"bot"}}""", requests.last { it.first.endsWith("/chat-tool-servers") }.second)
    }

    private fun connectKnownAddress(kind: String, label: String) {
        val store = store()
        rule.setContent { MyceliumTheme(true) { SetupFlow(store, kind, "https://gateway.test", emptySet(), {}, { finished = true }, {}, {}) } }

        rule.onNodeWithContentDescription(label).performTextInput("secret-$kind")
        rule.onNodeWithText("Continue").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connected. The server offers 2 tools.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()
        rule.waitUntil(5_000) { finished }
    }

    @Test
    fun outlineSendsItsKindWithTheMaskedApiKey() {
        connectKnownAddress("outline", "Outline API key")

        assertTrue(requests.last { it.first.endsWith("/chat-tool-servers") }.second.contains("\"kind\":\"outline\""))
        rule.onAllNodesWithText("secret-outline").assertCountEquals(0)
    }

    @Test
    fun liteLlmSendsItsKindWithTheMaskedKey() {
        connectKnownAddress("litellm", "Token")

        assertTrue(requests.last { it.first.endsWith("/chat-tool-servers") }.second.contains("\"kind\":\"litellm\""))
        rule.onAllNodesWithText("secret-litellm").assertCountEquals(0)
    }
}
