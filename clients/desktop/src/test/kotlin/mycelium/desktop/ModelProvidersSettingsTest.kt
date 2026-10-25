package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
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
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.SettingsScreen
import mycelium.ui.SettingsSection
import mycelium.ui.ModelProvidersSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val OllamaJson = """{"id":"p1","kind":"openai_compatible","name":"Ollama","base_url":"http://box:11434/v1","models":["llama3.2","qwen3"],"created_at":"2026-10-22T10:00:00Z"}"""

private const val AnthropicJson = """{"id":"p2","kind":"anthropic","name":"Claude API","key_hint":"wxyz","models":["claude-sonnet-5-5","claude-opus-5-5"],"default_model":"claude-sonnet-5-5","created_at":"2026-10-22T10:00:00Z"}"""

class ModelProvidersSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<HttpRequestData>())
    private val bodies = java.util.Collections.synchronizedList(mutableListOf<String>())
    private var listed = OllamaJson
    private var postAnswer: Pair<HttpStatusCode, String> = HttpStatusCode.Created to AnthropicJson

    private fun show(inSettings: Boolean = false) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += request
            bodies += String(request.body.toByteArray())

            when {
                request.method == HttpMethod.Post -> respond(postAnswer.second, postAnswer.first, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                else -> respond("[$listed]", HttpStatusCode.OK, json)
            }
        }

        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        rule.setContent {
            MyceliumTheme(true) {
                if (inSettings) {
                    SettingsScreen(store, Me("u", "reader@example.test", "Reader"), "https://mycelium.test", startOn = SettingsSection.Accounts, onSignOut = {})
                } else {
                    ModelProvidersSettings(store)
                }
            }
        }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Ollama").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun providersOpensChatModelsWithinTheGroupedSettingsNavigation() {
        show(inSettings = true)

        rule.onNodeWithText("Providers").assertExists()
        rule.onNodeWithText("Agents and models").assertExists()
        rule.onNodeWithText("Knowledge").assertExists()
        rule.onNodeWithText("Services").assertExists()
        rule.onNodeWithText("Your account").assertExists()
        rule.onNodeWithText("Chat models").assertExists()
        rule.onNodeWithText("Ollama").assertExists()
        rule.onNodeWithText("Agent connections").assertExists()
        rule.onNodeWithText("Integrations").assertExists()
    }

    @Test
    fun theListShowsEachProviderWithItsModels() {
        show()

        rule.onNodeWithText("Ollama").assertExists()
        rule.onNodeWithText("2 models").assertExists()
        rule.onNodeWithText("llama3.2").assertDoesNotExist()
        rule.onNodeWithContentDescription("Browse models from Ollama").performClick()
        rule.onNodeWithText("llama3.2").assertExists()
        rule.onNodeWithText("No key").assertExists()
    }

    @Test
    fun hundredsOfModelsStayCollapsedAndCanBeFoundBySearch() {
        val models = (1..500).joinToString(",") { "\"model-$it\"" }
        listed = OllamaJson.replace("\"llama3.2\",\"qwen3\"", models)
        show()

        rule.onNodeWithText("500 models").assertExists()
        rule.onNodeWithText("model-500").assertDoesNotExist()
        rule.onNodeWithContentDescription("Browse models from Ollama").performClick()
        rule.onNodeWithContentDescription("Search Ollama models").performTextInput("model-500")
        rule.onNode(hasText("model-500") and hasSetTextAction().not()).assertExists()
        rule.onNodeWithText("model-1").assertDoesNotExist()
    }

    @Test
    fun addingAnOpenAiCompatibleEndpointSendsItsBaseUrlAndNoKey() {
        postAnswer = HttpStatusCode.Created to OllamaJson
        show()

        rule.onNodeWithText("Add a provider").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Ollama")
        rule.onNodeWithContentDescription("Base URL").performTextInput("http://box:11434/v1")
        rule.onNodeWithText("Ollama serves this at http://<host>:11434/v1.").assertExists()
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Base URL").fetchSemanticsNodes().isEmpty() }
        assertEquals("""{"kind":"openai_compatible","name":"Ollama","base_url":"http://box:11434/v1"}""", bodies.last { it.isNotEmpty() })
    }

    @Test
    fun addingAnAnthropicProviderSendsTheMaskedKeyAndShowsOnlyItsHint() {
        show()

        rule.onNodeWithText("Add a provider").performClick()
        rule.onNodeWithText("Anthropic").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Claude API")
        rule.onNodeWithContentDescription("API key").performTextInput("sk-ant-secret")
        rule.onNodeWithContentDescription("API key").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Ends in wxyz").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"kind":"anthropic","name":"Claude API","api_key":"sk-ant-secret"}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("sk-ant-secret").assertCountEquals(0)
    }

    @Test
    fun addingOpenRouterSendsItsAddressWithTheKeyAndAsksForAKeyFirst() {
        show()

        rule.onNodeWithText("Add a provider").performClick()
        rule.onNodeWithText("OpenRouter").performClick()
        rule.onNodeWithText("Add provider").performClick()
        rule.onNodeWithText("Enter your OpenRouter API key.").assertExists()

        rule.onNodeWithContentDescription("API key").performTextInput("sk-or-secret")
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil(5_000) { bodies.any { it.contains("openrouter.ai") } }
        assertEquals("""{"kind":"openai_compatible","name":"OpenRouter","base_url":"https://openrouter.ai/api/v1","api_key":"sk-or-secret"}""", bodies.last { it.isNotEmpty() })
    }

    @Test
    fun aServerErrorShowsItsMessageAndKeepsTheForm() {
        postAnswer = HttpStatusCode.BadRequest to """{"error":"provider_unreachable","message":"Mycelium could not reach http://box:11434/v1. Check the address and try again."}"""
        show()

        rule.onNodeWithText("Add a provider").performClick()
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Mycelium could not reach http://box:11434/v1. Check the address and try again.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Base URL").assertExists()
    }

    @Test
    fun refreshingFetchesTheModelsAgain() {
        postAnswer = HttpStatusCode.OK to OllamaJson.replace("llama3.2", "gemma3")
        show()

        rule.onNodeWithContentDescription("More for Ollama").performClick()
        rule.onNodeWithText("Refresh models").performClick()
        rule.onNodeWithContentDescription("Browse models from Ollama").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("gemma3").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("/api/v1/model-providers/p1/refresh", requests.last { it.method == HttpMethod.Post }.url.encodedPath)
    }

    @Test
    fun removingDeletesTheProviderAndItsRow() {
        show()

        rule.onNodeWithContentDescription("More for Ollama").performClick()
        rule.onNodeWithText("Remove provider").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Ollama").fetchSemanticsNodes().isEmpty() }
        assertEquals("/api/v1/model-providers/p1", requests.last { it.method == HttpMethod.Delete }.url.encodedPath)
    }
}
