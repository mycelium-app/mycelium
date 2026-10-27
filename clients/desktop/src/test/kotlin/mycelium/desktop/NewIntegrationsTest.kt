package mycelium.desktop

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
import kotlin.test.assertTrue

private const val Added = """{"id":"n1","name":"Added","kind":"freeagent","url":"https://api.freeagent.com","account":"id-1","enabled":true,"status":"needs_sign_in","tools":[],"created_at":"2026-10-27T09:00:00Z","policy":{"preset":"ask_writes"}}"""

class NewIntegrationsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Pair<String, String>>()
    private var finished = false

    private fun open(kind: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += path to String(request.body.toByteArray())

            when {
                path.endsWith("/freeagent-sign-in") -> respond("""{"authorize_url":"https://api.freeagent.com/v2/approve_app"}""", HttpStatusCode.OK, json)
                request.method.value == "POST" -> respond(Added, HttpStatusCode.Created, json)
                else -> respond("[]", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        rule.setContent { MyceliumTheme(true) { SetupFlow(store, kind, null, emptySet(), {}, { finished = true }, {}, {}) } }
    }

    private fun type(label: String, text: String) = rule.onNodeWithContentDescription(label).performTextInput(text)

    private fun waitForLabel(label: String) = rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription(label).fetchSemanticsNodes().isNotEmpty() }

    private fun created(): String = requests.last { it.first.endsWith("/chat-tool-servers") }.second

    @Test
    fun discordNeedsABotTokenAndSendsItWithTheNameMessagesAreSignedWith() {
        open("discord")

        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Enter the name people know you by on Discord.").assertExists()
        type("Your name on Discord", "Sam")
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Enter your Discord bot token.").assertExists()
        type("Discord bot token", "bot-token")
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { finished }
        assertTrue(created().contains("\"kind\":\"discord\"") && created().contains("\"account\":\"Sam\"") && created().contains("\"bearer_token\":\"bot-token\""), created())
    }

    @Test
    fun actualBudgetSaysABridgeIsNeededAndSendsTheBudgetSyncId() {
        open("actual")

        rule.onNodeWithText("Actual Budget has no HTTP API of its own", substring = true).assertExists()
        type("Actual HTTP API address", "http://bridge.local:5007")
        rule.onNodeWithText("Continue").performClick()
        waitForLabel("Budget sync id")
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Enter your budget's sync id, from Settings, Show advanced settings in Actual Budget.").assertExists()
        type("Budget sync id", "sync-1")
        type("Actual HTTP API key", "api-key")
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { finished }
        assertTrue(created().contains("\"budget_sync_id\":\"sync-1\"") && created().contains("\"url\":\"http://bridge.local:5007\""), created())
    }

    @Test
    fun paperlessAsksForTheTokenBeforeMovingOn() {
        open("paperless")

        type("Paperless-ngx address", "https://paperless.example.com")
        rule.onNodeWithText("Continue").performClick()
        waitForLabel("Paperless-ngx API token")
        rule.onNodeWithText("Continue").performClick()

        rule.onNodeWithText("Enter your Paperless-ngx API token.").assertExists()
        assertTrue(requests.none { it.first.endsWith("/chat-tool-servers") })
    }

    @Test
    fun freeAgentIsAddedThenOffersSigningIn() {
        open("freeagent")

        type("FreeAgent OAuth identifier", "id-1")
        type("FreeAgent OAuth secret", "secret-1")
        rule.onNodeWithText("Sandbox").performClick()
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Sign in to FreeAgent").fetchSemanticsNodes().isNotEmpty() }
        assertTrue(created().contains("\"url\":\"https://api.sandbox.freeagent.com\"") && created().contains("\"account\":\"id-1\""), created())
        rule.onNodeWithText("Sign in to FreeAgent").performClick()
        rule.waitUntil(5_000) { requests.any { it.first.endsWith("/n1/freeagent-sign-in") } }
    }
}
