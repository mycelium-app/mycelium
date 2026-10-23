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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import mycelium.api.MyceliumApi
import mycelium.state.Store
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

    private val requests = mutableListOf<HttpRequestData>()
    private val bodies = mutableListOf<String>()
    private var postAnswer: Pair<HttpStatusCode, String> = HttpStatusCode.Created to AnthropicJson

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += request
            bodies += String(request.body.toByteArray())

            when {
                request.method == HttpMethod.Post -> respond(postAnswer.second, postAnswer.first, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                else -> respond("[$OllamaJson]", HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
    }

    @Test
    fun theListShowsEachProviderWithItsModels() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.onNodeWithText("Ollama").assertExists()
        rule.onNodeWithText("llama3.2, qwen3").assertExists()
        rule.onNodeWithText("No key").assertExists()
    }

    @Test
    fun addingAnOpenAiCompatibleEndpointSendsItsBaseUrlAndNoKey() {
        postAnswer = HttpStatusCode.Created to OllamaJson
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.onNodeWithText("Add a provider").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Ollama")
        rule.onNodeWithContentDescription("Base URL").performTextInput("http://box:11434/v1")
        rule.onNodeWithText("Ollama serves this at http://<host>:11434/v1.").assertExists()
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil { rule.onAllNodesWithText("Add a provider").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"kind":"openai_compatible","name":"Ollama","base_url":"http://box:11434/v1"}""", bodies.last { it.isNotEmpty() })
    }

    @Test
    fun addingAnAnthropicProviderSendsTheMaskedKeyAndShowsOnlyItsHint() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.onNodeWithText("Add a provider").performClick()
        rule.onNodeWithText("Anthropic").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Claude API")
        rule.onNodeWithContentDescription("API key").performTextInput("sk-ant-secret")
        rule.onNodeWithContentDescription("API key").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil { rule.onAllNodesWithText("Ends in wxyz").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"kind":"anthropic","name":"Claude API","api_key":"sk-ant-secret"}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("sk-ant-secret").assertCountEquals(0)
    }

    @Test
    fun aServerErrorShowsItsMessageAndKeepsTheForm() {
        postAnswer = HttpStatusCode.BadRequest to """{"error":"provider_unreachable","message":"Mycelium could not reach http://box:11434/v1. Check the address and try again."}"""
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.onNodeWithText("Add a provider").performClick()
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil { rule.onAllNodesWithText("Mycelium could not reach http://box:11434/v1. Check the address and try again.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Base URL").assertExists()
    }

    @Test
    fun refreshingFetchesTheModelsAgain() {
        postAnswer = HttpStatusCode.OK to OllamaJson.replace("llama3.2", "gemma3")
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.onNodeWithContentDescription("Refresh the models of Ollama").performClick()

        rule.waitUntil { rule.onAllNodesWithText("gemma3, qwen3").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("/api/v1/model-providers/p1/refresh", requests.last { it.method == HttpMethod.Post }.url.encodedPath)
    }

    @Test
    fun removingDeletesTheProviderAndItsRow() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { ModelProvidersSettings(store) } }

        rule.onNodeWithContentDescription("Remove Ollama").performClick()

        rule.waitUntil { rule.onAllNodesWithText("llama3.2, qwen3").fetchSemanticsNodes().isEmpty() }
        assertEquals("/api/v1/model-providers/p1", requests.last { it.method == HttpMethod.Delete }.url.encodedPath)
    }
}
