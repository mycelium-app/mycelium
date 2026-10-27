package mycelium.desktop

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
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.DecisionsSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val JevJson = """{"id":"d1","name":"OpenJev","base_url":"http://gpu-box.example:8080","model":"openjev-latest","uses":["decide"],"created_at":"2026-10-27T10:00:00Z"}"""

class DecisionsSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posted = CopyOnWriteArrayList<Pair<String, String>>()
    private var listed = "[$JevJson]"

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val body = String(request.body.toByteArray())

            when {
                request.method == HttpMethod.Post && request.url.encodedPath.endsWith("/test") -> {
                    posted += "test" to body
                    respond("""{"models":["openjev-latest","diffusiongemma-26b"]}""", HttpStatusCode.OK, json)
                }

                request.method == HttpMethod.Post -> {
                    posted += "create" to body
                    respond(JevJson.replace("OpenJev", "Home"), HttpStatusCode.Created, json)
                }

                else -> respond(listed, HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    @Test
    fun theSectionListsEachProviderWithItsModelAndWhatItIsUsedFor() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { DecisionsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("OpenJev").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("openjev-latest").assertExists()
        rule.onNodeWithText("decide").assertExists()
        rule.onNodeWithText("No key").assertExists()
    }

    @Test
    fun testingTheConnectionFillsTheModelMenuAndSaveSendsTheChosenUses() {
        listed = "[]"
        val store = store()
        rule.setContent { MyceliumTheme(true) { DecisionsSettings(store) } }

        rule.onNodeWithText("Add a decisions provider").performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("Home")
        rule.onNodeWithContentDescription("Address").performTextInput("http://gpu-box.example:8080")
        rule.onNodeWithText("Test").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connected. The server offers 2 models.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { posted.any { it.first == "create" } }
        assertEquals("""{"name":"Home","base_url":"http://gpu-box.example:8080","model":"openjev-latest","uses":["decide"]}""", posted.first { it.first == "create" }.second)
    }

    @Test
    fun aFailedTestKeepsTheDialogOpenAndExplainsWhy() {
        listed = "[]"
        val failing = MockEngine { respond("""{"error":"decision_provider_failed","message":"The server refused that key. Check it was copied whole and has not been revoked."}""", HttpStatusCode.BadRequest, headersOf(HttpHeaders.ContentType, "application/json")) }
        val store = Store(MyceliumApi("https://mycelium.test", "token", failing), scopes.create())
        rule.setContent { MyceliumTheme(true) { DecisionsSettings(store) } }

        rule.onNodeWithText("Add a decisions provider").performClick()
        rule.onNodeWithText("Test").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("The server refused that key", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText("Save").assertCountEquals(1)
        assertTrue(posted.isEmpty())
    }
}

class DecisionsSettingsRenderTest {
    @Test
    fun theDecisionsSectionAndItsDialogRender() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { respond("[$JevJson]", HttpStatusCode.OK, json) }
        val scopesRule = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Default)
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopesRule)
        val section = renderAt("decisions-settings", true, false) { DecisionsSettings(store) }

        assertTrue(section.length() > 2_000)
    }
}
