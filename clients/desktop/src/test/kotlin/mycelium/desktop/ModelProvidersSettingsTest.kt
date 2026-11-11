package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextClearance
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

private const val AnthropicWithAdminKeyJson = """{"id":"p2","kind":"anthropic","name":"Claude API","key_hint":"wxyz","has_admin_key":true,"admin_key_hint":"9c1e","models":["claude-sonnet-5-5"],"default_model":"claude-sonnet-5-5","created_at":"2026-10-22T10:00:00Z"}"""

private val ChatModelsAddProvider = hasText("Add a provider").and(hasContentDescription("Add a provider for speech to text").not())

class ModelProvidersSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.concurrent.CopyOnWriteArrayList<HttpRequestData>()
    private val bodies = java.util.concurrent.CopyOnWriteArrayList<String>()
    private var listed = OllamaJson
    private var postAnswer: Pair<HttpStatusCode, String> = HttpStatusCode.Created to AnthropicJson
    private var patchAnswer: Pair<HttpStatusCode, String>? = null

    private fun show(experiments: String = "{}", waitFor: String = "Ollama") {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            bodies += String(request.body.toByteArray())
            requests += request

            when {
                request.url.encodedPath.endsWith("/experiments") -> respond(experiments, HttpStatusCode.OK, json)
                request.method == HttpMethod.Post -> respond(postAnswer.second, postAnswer.first, json)
                request.method == HttpMethod.Patch -> patchAnswer?.let { respond(it.second, it.first, json) } ?: respond(OllamaJson.replace("\"Ollama\"", "\"Workstation\""), HttpStatusCode.OK, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                else -> respond("[$listed]", HttpStatusCode.OK, json)
            }
        }

        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        kotlinx.coroutines.runBlocking { store.refreshExperiments() }
        rule.setContent {
            MyceliumTheme(true) {
                ModelProvidersSettings(store)
            }
        }
        rule.waitUntil(5_000) { rule.onAllNodesWithText(waitFor).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aProvidersDataHandlingIsSaved() {
        show(experiments = EnabledExperiments)
        rule.onNodeWithContentDescription("Data handling: Cloud").performClick()
        rule.onNodeWithText("Private or on-device only").performClick()
        rule.waitUntil(5_000) { requests.any { it.method == HttpMethod.Patch } }

        assertEquals(true, bodies.any { it.contains("\"model_class\":\"private\"") })
    }

    @Test
    fun dataHandlingIsOfferedWithNoExperimentOn() {
        show()
        rule.onNodeWithContentDescription("Data handling: Cloud").assertExists()
    }

    @Test
    fun theListShowsEachProviderWithItsModels() {
        show()

        rule.onNodeWithText("Ollama").assertExists()
        rule.onNodeWithText("llama3.2").assertDoesNotExist()
        rule.onNodeWithContentDescription("Browse models from Ollama").performClick()
        rule.onNodeWithText("llama3.2").assertExists()
    }

    @Test
    fun hundredsOfModelsStayCollapsedAndCanBeFoundBySearch() {
        val models = (1..500).joinToString(",") { "\"model-$it\"" }
        listed = OllamaJson.replace("\"llama3.2\",\"qwen3\"", models)
        show()

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

        rule.onNode(ChatModelsAddProvider).performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Ollama")
        rule.onNodeWithContentDescription("Base URL").performTextInput("http://box:11434/v1")
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Base URL").fetchSemanticsNodes().isEmpty() }
        assertEquals("""{"kind":"openai_compatible","name":"Ollama","base_url":"http://box:11434/v1"}""", bodies.last { it.isNotEmpty() })
    }

    @Test
    fun addingAnAnthropicProviderSendsTheMaskedKeyAndShowsOnlyItsHint() {
        show()

        rule.onNode(ChatModelsAddProvider).performClick()
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

        rule.onNode(ChatModelsAddProvider).performClick()
        rule.onNodeWithText("OpenRouter").performClick()
        rule.onNodeWithText("Add provider").performClick()
        rule.onNodeWithText("Enter your OpenRouter API key.").assertExists()

        rule.onNodeWithContentDescription("API key").performTextInput("sk-or-secret")
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil(5_000) { bodies.any { it.contains("openrouter.ai") } }
        assertEquals("""{"kind":"openai_compatible","name":"OpenRouter","base_url":"https://openrouter.ai/api/v1","api_key":"sk-or-secret"}""", bodies.last { it.isNotEmpty() })
    }

    @Test
    fun addingLiteLlmSendsItsAddressAsOpenAiCompatibleWithTheKey() {
        show()

        rule.onNode(ChatModelsAddProvider).performClick()
        rule.onNodeWithText("LiteLLM").performClick()
        rule.onNodeWithContentDescription("Base URL").performTextInput("http://proxy:4000")
        rule.onNodeWithContentDescription("API key, if the proxy needs one").performTextInput("sk-1")
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil(5_000) { bodies.any { it.contains("proxy:4000") } }
        assertEquals("""{"kind":"openai_compatible","name":"LiteLLM","base_url":"http://proxy:4000","api_key":"sk-1"}""", bodies.last { it.isNotEmpty() })
    }

    @Test
    fun aServerErrorShowsItsMessageAndKeepsTheForm() {
        postAnswer = HttpStatusCode.BadRequest to """{"error":"provider_unreachable","message":"Mycelium could not reach http://box:11434/v1. Check the address and try again."}"""
        show()

        rule.onNode(ChatModelsAddProvider).performClick()
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("could not reach", substring = true).fetchSemanticsNodes().isNotEmpty() }
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
    fun renamingSendsTheTrimmedNameAndShowsItInTheList() {
        show()

        rule.onNodeWithContentDescription("More for Ollama").performClick()
        rule.onNodeWithText("Rename").performClick()
        rule.onNodeWithContentDescription("Provider name").performTextClearance()
        rule.onNodeWithContentDescription("Provider name").performTextInput("  Workstation ")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Workstation").fetchSemanticsNodes().isNotEmpty() }
        val patch = requests.indexOfLast { it.method == HttpMethod.Patch }
        assertEquals("/api/v1/model-providers/p1", requests[patch].url.encodedPath)
        assertEquals("""{"name":"Workstation"}""", bodies[patch])
        rule.onNodeWithText("Ollama").assertDoesNotExist()
    }

    @Test
    fun removingDeletesTheProviderAndItsRow() {
        show()

        rule.onNodeWithContentDescription("More for Ollama").performClick()
        rule.onNodeWithText("Remove provider").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Ollama").fetchSemanticsNodes().isEmpty() }
        assertEquals("/api/v1/model-providers/p1", requests.last { it.method == HttpMethod.Delete }.url.encodedPath)
    }

    @Test
    fun anAnthropicRowShowsTheAdminKeyHintUnderTheKeyAndNeverTheKey() {
        listed = AnthropicWithAdminKeyJson
        show(waitFor = "Claude API")

        rule.onNodeWithText("Ends in wxyz").assertExists()
        rule.onNodeWithText("Admin key ...9c1e").assertExists()
    }

    @Test
    fun theMenuOffersToAddAnAdminKeyOnAnthropicOnly() {
        listed = "$AnthropicJson,$OllamaJson"
        show()

        rule.onNodeWithContentDescription("More for Claude API").performClick()
        rule.onNodeWithText("Add admin key").assertExists()
        rule.onNodeWithText("Remove admin key").assertDoesNotExist()
    }

    @Test
    fun theMenuOffersToChangeOrRemoveAnExistingAdminKey() {
        listed = AnthropicWithAdminKeyJson
        patchAnswer = HttpStatusCode.OK to AnthropicJson
        show(waitFor = "Claude API")

        rule.onNodeWithContentDescription("More for Claude API").performClick()
        rule.onNodeWithText("Change admin key").assertExists()
        rule.onNodeWithText("Remove admin key").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Admin key ...9c1e").fetchSemanticsNodes().isEmpty() }
        assertEquals("""{"admin_key":""}""", bodies.last { it.isNotEmpty() })
    }

    @Test
    fun aKeyWithoutTheAdminPrefixIsExplainedInASummaryAndSendsNothing() {
        listed = AnthropicJson
        show(waitFor = "Claude API")

        rule.onNodeWithContentDescription("More for Claude API").performClick()
        rule.onNodeWithText("Add admin key").performClick()
        rule.onNodeWithContentDescription("Admin API key (optional)").performTextInput("sk-ant-api03-not-admin")
        rule.onNodeWithContentDescription("Admin API key (optional)").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        rule.onNodeWithText("Save").performClick()

        rule.onNodeWithText("There is a problem").assertExists()
        rule.onNodeWithContentDescription("Go to Admin API key: Enter an Admin API key that starts with sk-ant-admin").assertExists()
        assertEquals(false, requests.any { it.method == HttpMethod.Patch })
    }

    @Test
    fun anAdminKeyAnthropicRefusesIsShownItsMessageAndTheDialogStays() {
        listed = AnthropicJson
        patchAnswer = HttpStatusCode.BadRequest to """{"error":"admin_key_refused","message":"Anthropic refused this Admin API key. Check it is active in the Console, or create a new one"}"""
        show(waitFor = "Claude API")

        rule.onNodeWithContentDescription("More for Claude API").performClick()
        rule.onNodeWithText("Add admin key").performClick()
        rule.onNodeWithContentDescription("Admin API key (optional)").performTextInput("sk-ant-admin01-example")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Anthropic refused", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(hasContentDescription("Admin API key (optional). Error: Anthropic refused", substring = true)).assertExists()
    }

    @Test
    fun savingAnAdminKeySendsItAndShowsOnlyItsHint() {
        listed = AnthropicJson
        patchAnswer = HttpStatusCode.OK to AnthropicWithAdminKeyJson
        show(waitFor = "Claude API")

        rule.onNodeWithContentDescription("More for Claude API").performClick()
        rule.onNodeWithText("Add admin key").performClick()
        rule.onNodeWithContentDescription("Admin API key (optional)").performTextInput("sk-ant-admin01-example")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Admin key ...9c1e").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"admin_key":"sk-ant-admin01-example"}""", bodies.last { it.isNotEmpty() })
        rule.onAllNodesWithText("sk-ant-admin01-example").assertCountEquals(0)
    }

    @Test
    fun theAddFormTakesAnOptionalAdminKeyForAnthropicOnly() {
        show()

        rule.onNode(ChatModelsAddProvider).performClick()
        rule.onAllNodesWithContentDescription("Admin API key (optional)").assertCountEquals(0)
        rule.onNodeWithText("Anthropic").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Claude API")
        rule.onNodeWithContentDescription("API key").performTextInput("sk-ant-secret")
        rule.onNodeWithContentDescription("Admin API key (optional)").performTextInput("sk-ant-admin01-example")
        rule.onNodeWithText("Add provider").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Ends in wxyz").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"kind":"anthropic","name":"Claude API","api_key":"sk-ant-secret","admin_key":"sk-ant-admin01-example"}""", bodies.last { it.isNotEmpty() })
    }
}
