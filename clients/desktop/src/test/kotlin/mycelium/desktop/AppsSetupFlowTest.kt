package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpStatusCode
import java.util.concurrent.CopyOnWriteArrayList
import mycelium.ui.MyceliumTheme
import mycelium.ui.SetupFlow
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private const val CoolifyTested = """{"host":"coolify.example.com","version":"4.0.0","projects":[{"uuid":"p1","name":"Mycelium apps"},{"uuid":"p2","name":"Homelab"}],"servers":[{"uuid":"s1","name":"atlas-box","wildcard_domain":"https://apps.example.com"}]}"""

private const val TandoorTested = """{"host":"recipes.example.com","space":"Home","user":"sam","recipes":214}"""

class AppsSetupFlowTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Pair<String, String>>()
    private var finished = false
    private var coolifyTest: Pair<HttpStatusCode, String> = HttpStatusCode.OK to CoolifyTested
    private var tandoorTest: Pair<HttpStatusCode, String> = HttpStatusCode.OK to TandoorTested

    private fun open(kind: String) {
        val store = appsStore(scopes.create()) { request ->
            val path = request.url.encodedPath
            requests += path to String(request.body.toByteArray())

            when {
                path.endsWith("/coolify-test") -> json(coolifyTest.second, coolifyTest.first)
                path.endsWith("/tandoor-test") -> json(tandoorTest.second, tandoorTest.first)
                request.method.value == "POST" -> json(CoolifyAccount, HttpStatusCode.Created)
                else -> json("[]")
            }
        }

        rule.setContent { MyceliumTheme(true) { SetupFlow(store, kind, null, emptySet(), {}, { finished = true }, {}, {}) } }
    }

    private fun type(label: String, text: String) = rule.onNodeWithContentDescription(label).performTextInput(text)

    private fun waitForText(text: String) = rule.waitUntil(5_000) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }

    @Test
    fun coolifyAsksForTheAddressThenTheTokenAndShowsWhereToCreateIt() {
        open("coolify")

        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Enter the address you open Coolify at, like https://coolify.example.com").assertExists()
        rule.onNodeWithContentDescription("Server address", substring = true).performTextInput("https://coolify.example.com")
        rule.onNodeWithText("Continue").performClick()

        rule.onNodeWithText("In Coolify, open Keys & Tokens, then API Tokens.").assertExists()
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Enter your Coolify API token.").assertExists()
        assertTrue(requests.none { it.first.endsWith("/coolify-test") })
    }

    @Test
    fun coolifyTestShowsTheHostProjectsAndServersThenNamesTheAccountWithoutChoosingWhereAppsGo() {
        open("coolify")

        type("Server address", "https://coolify.example.com")
        rule.onNodeWithText("Continue").performClick()
        type("API token", "coolify-token")
        rule.onNodeWithText("Continue").performClick()

        waitForText("Connected to coolify.example.com.")
        rule.onNodeWithText("Mycelium apps").assertExists()
        rule.onNodeWithText("atlas-box").assertExists()
        rule.onNodeWithText("Continue").performClick()

        rule.onNodeWithContentDescription("Step 4 of 4: Name", substring = true).assertExists()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { finished }
        val created = requests.last { it.first.endsWith("/chat-tool-servers") }.second
        assertTrue(created.contains("\"kind\":\"coolify\"") && created.contains("\"bearer_token\":\"coolify-token\"") && !created.contains("project_uuid"), created)
    }

    @Test
    fun coolifyShowsTheServersMessageWhenTheTokenIsRefused() {
        coolifyTest = HttpStatusCode.BadRequest to """{"error":"coolify_token","message":"coolify.example.com did not accept the API token. Check it has the read, write and deploy permissions, or create a new one."}"""
        open("coolify")

        type("Server address", "https://coolify.example.com")
        rule.onNodeWithText("Continue").performClick()
        type("API token", "bad")
        rule.onNodeWithText("Continue").performClick()

        waitForText("coolify.example.com did not accept the API token. Check it has the read, write and deploy permissions, or create a new one.")
        rule.onNodeWithText("Back to credential").assertExists()
        rule.onNodeWithText("Try again").assertExists()
        rule.onNodeWithText("Back to credential").performClick()
        rule.onNodeWithText("In Coolify, open Keys & Tokens, then API Tokens.").assertExists()
    }

    @Test
    fun tandoorRejectsAnAddressWithoutASchemeBeforeAskingTheServer() {
        open("tandoor")

        type("Server address", "recipes.example.com")
        type("API token", "tandoor-token")
        rule.onNodeWithText("Continue").performClick()

        rule.onNodeWithText("Enter the server address, like https://recipes.example.com").assertExists()
        assertTrue(requests.none { it.first.endsWith("/tandoor-test") })
    }

    @Test
    fun tandoorTestNamesTheSpaceTheUserAndTheRecipeCount() {
        open("tandoor")

        type("Server address", "https://recipes.example.com")
        type("API token", "tandoor-token")
        rule.onNodeWithText("Continue").performClick()

        waitForText("Connected to the Home space as sam. Found 214 recipes.")
        rule.onNodeWithText("Meal plans").assertExists()
        rule.onNodeWithText("Shopping list").assertExists()
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()

        rule.waitUntil(5_000) { finished }
        val created = requests.last { it.first.endsWith("/chat-tool-servers") }.second
        assertTrue(created.contains("\"kind\":\"tandoor\"") && created.contains("\"bearer_token\":\"tandoor-token\"") && created.contains("\"token_expires_at\""), created)
    }

    @Test
    fun tandoorShowsTheServersMessageAndOffersBothWaysBack() {
        tandoorTest = HttpStatusCode.BadRequest to """{"error":"tandoor_address","message":"Could not reach recipes.example.com. Check the address and that Tandoor is running."}"""
        open("tandoor")

        type("Server address", "https://recipes.example.com")
        type("API token", "tandoor-token")
        rule.onNodeWithText("Continue").performClick()

        waitForText("Could not reach recipes.example.com. Check the address and that Tandoor is running.")
        rule.onNodeWithText("Back to address and token").assertExists()
        rule.onNodeWithText("Try again").assertExists()
    }
}
