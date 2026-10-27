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
    fun aWaitingGateShowsItsDecisionAndContinuesWhenApproved() {
        val calls = java.util.Collections.synchronizedList(mutableListOf<String>())
        val fixture = """{"id":"w1","parent_id":null,"title":"Review","status":"running","round":1,"plan":{"steps":[{"key":"build","title":"Build","agent":"codex","model":"build-model","difficulty":"medium"},{"key":"design","title":"Approve design","prompt":"Review the implementation before starting independent checks.","depends_on":["build"]},{"key":"review","title":"Review accessibility","depends_on":["design"]},{"key":"security","title":"Review security","depends_on":["design"],"workflow":{"title":"Security"}}]},"steps":[{"key":"build","status":"done","conversation_id":"c1","workflow_id":null,"output":"Implementation ready for review."},{"key":"design","status":"gate","conversation_id":null,"workflow_id":null,"output":""},{"key":"review","status":"waiting","conversation_id":null,"workflow_id":null,"output":""},{"key":"security","status":"waiting","conversation_id":null,"workflow_id":null,"output":""}]}"""
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            if (request.url.encodedPath.endsWith("/experiments")) {
                respond(EnabledExperiments, HttpStatusCode.OK, json)
            } else if (request.method == HttpMethod.Post) {
                calls += String(request.body.toByteArray())
                respond(fixture, HttpStatusCode.OK, json)
            } else {
                respond("[$fixture]", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "test-token", engine), scopes.create())
        kotlinx.coroutines.runBlocking { store.refreshExperiments() }
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); WorkflowsScreen(store, state, {}) } }
        rule.waitUntil(5000) { rule.onAllNodesWithText("Review · running · Round 1").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Review · running · Round 1").performClick()
        rule.onNodeWithContentDescription("Decision for design").performTextInput("Use option B")
        rule.onNodeWithText("Approve and continue").performScrollTo().performClick()
        rule.waitUntil(5000) { calls.isNotEmpty() }
        val decision = Json.parseToJsonElement(calls.single()).jsonObject
        assertEquals("\"approve\"", decision["action"].toString())
        assertEquals("\"design\"", decision["step"].toString())
        assertEquals("\"Use option B\"", decision["output"].toString())
    }
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
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); WorkflowsScreen(store, state, {}) } }
        rule.onNodeWithText("New workflow").performClick()
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
