package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MemoriesSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val StoredMemory = """{"id":"m1","text":"Staging listens on 5433","scopes":["global"],"created_by":"laptop/Fix login","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val HealthBank = """{"id":"b1","name":"Health","model_class":"private","group_ids":[],"created_at":"2026-10-22T10:00:00Z"}"""

class MemoryBanksTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val calls = java.util.Collections.synchronizedList(mutableListOf<String>())

    private fun show(experiments: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val body = String(request.body.toByteArray())
            calls += "${request.method.value} ${request.url.encodedPath} $body"

            when {
                request.url.encodedPath.endsWith("/experiments") -> respond(experiments, HttpStatusCode.OK, json)
                request.url.encodedPath.endsWith("/memory-banks") && request.method == HttpMethod.Post -> respond(HealthBank, HttpStatusCode.Created, json)
                request.url.encodedPath.endsWith("/memory-banks") -> respond("[]", HttpStatusCode.OK, json)
                request.url.encodedPath.endsWith("/bank") -> respond(StoredMemory.replace("\"scopes\"", "\"bank_id\":\"b1\",\"scopes\""), HttpStatusCode.OK, json)
                request.url.encodedPath.endsWith("/groups") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("[$StoredMemory]", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refreshExperiments() }
        rule.setContent { MyceliumTheme(true) { MemoriesSettings(store) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Staging listens on 5433").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun withMemoryBanksOnABankIsCreatedAndAMemoryMovedIntoIt() {
        show(EnabledExperiments)
        rule.onNodeWithText("New bank").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Health")
        rule.onNodeWithText("Create bank").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Private or on-device only").fetchSemanticsNodes().isNotEmpty() || calls.any { it.startsWith("POST") } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Every group").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("More for Staging listens on 5433").performClick()
        rule.onNodeWithText("Move to bank").performClick()
        rule.onNodeWithContentDescription("Bank: No bank").performClick()
        rule.onAllNodesWithText("Health")[1].performClick()
        rule.onNodeWithText("Move memory").performClick()
        rule.waitUntil(5_000) { calls.any { it.startsWith("PUT /api/v1/memories/m1/bank") } }

        assertTrue(calls.any { it.startsWith("POST /api/v1/memory-banks") && it.contains("\"name\":\"Health\"") })
        assertEquals(true, calls.any { it.startsWith("PUT") && it.contains("\"bank_id\":\"b1\"") })
    }

    @Test
    fun memoryBanksAreOfferedWithNoExperimentOn() {
        show("{}")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("New bank").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for Staging listens on 5433").performClick()
        rule.onNodeWithText("Move to bank").assertExists()
    }
}
