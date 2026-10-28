package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonArray
import mycelium.api.MyceliumApi
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.WorkflowsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class WorkflowsTest {
    @get:Rule val rule = createComposeRule()
    @get:Rule val scopes = TestScopes()

    @Test
    fun aChatModelStepUsesItsSelectedProviderAndModel() {
        val requests = java.util.Collections.synchronizedList(mutableListOf<String>())
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                request.method == HttpMethod.Post -> {
                    requests += String(request.body.toByteArray())
                    """{"id":"w1"}"""
                }
                path.endsWith("/model-providers") -> """[{"id":"p1","kind":"openai","name":"Test provider","models":["chat-model"],"default_model":"chat-model","created_at":"2026-10-24T10:00:00Z"}]"""
                else -> "[]"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "test-token", engine), scopes.create())
        kotlinx.coroutines.runBlocking { store.refreshExperiments() }
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); WorkflowsScreen(store, state, {}, {}) } }
        rule.onAllNodesWithText("New workflow").onFirst().performClick()
        rule.onNodeWithContentDescription("Title").performTextInput("Review")
        rule.onNodeWithContentDescription("Step 1 title").performTextInput("Summary")
        rule.onNodeWithContentDescription("Step 1 prompt").performTextInput("Summarise the work")
        rule.onNodeWithText("Chat model").performScrollTo().performClick()
        rule.waitUntil(5000) { rule.onAllNodesWithText("Test provider").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Test provider").performScrollTo().performClick()
        rule.onNodeWithText("Start workflow").performScrollTo().performClick()
        rule.waitUntil(5000) { requests.isNotEmpty() }
        val step = Json.parseToJsonElement(requests.single()).jsonObject["steps"]!!.jsonArray.single().jsonObject
        assertEquals("\"p1\"", step["provider_id"].toString())
        assertEquals("\"chat-model\"", step["model"].toString())
        assertEquals(null, step["node_id"])
    }

}
