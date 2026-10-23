package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertTextEquals
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
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ForgesSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val GitHubJson = """{"id":"f1","kind":"github","base_url":"https://github.com","account":"samexample","token_hint":"wxyz",
"organisations":["samexample","acme-org"],"created_at":"2026-10-22T10:00:00Z"}"""

private const val GitLabJson = """{"id":"f2","kind":"gitlab","base_url":"https://gitlab.example.com","account":"sam","token_hint":"1234",
"organisations":["sam","slop"],"created_at":"2026-10-22T10:00:00Z"}"""

class ForgesSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<HttpRequestData>())
    private val bodies = java.util.Collections.synchronizedList(mutableListOf<String>())
    private var connectAnswer: Pair<HttpStatusCode, String> = HttpStatusCode.Created to GitLabJson

    private fun store(initial: String = "[$GitHubJson]"): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += request
            bodies += String(request.body.toByteArray())

            when {
                request.method == HttpMethod.Post -> respond(connectAnswer.second, connectAnswer.first, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                else -> respond(initial, HttpStatusCode.OK, json)
            }
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
    }

    private fun showSettings(store: Store) {
        rule.setContent { MyceliumTheme(true) { ForgesSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("samexample").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theListShowsEachForgesAccountHostAndOrganisations() {
        val store = store()
        showSettings(store)

        rule.onNodeWithText("GitHub").assertExists()
        rule.onNodeWithText("github.com").assertExists()
        rule.onNodeWithText("samexample").assertExists()
        rule.onNodeWithText("samexample, acme-org").assertExists()
    }

    @Test
    fun connectingSendsTheKindBaseUrlAndTokenAndAddsTheRow() {
        val store = store()
        showSettings(store)

        rule.onNodeWithText("Connect a forge").performClick()
        rule.onNodeWithText("GitLab").performClick()
        rule.onNodeWithContentDescription("Base URL for a self-hosted forge").performTextInput("https://gitlab.example.com")
        rule.onNodeWithContentDescription("Personal access token").performTextInput("glpat-secret")
        rule.onNodeWithText("Connect").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("sam, slop").fetchSemanticsNodes().isNotEmpty() }
        assertEquals("""{"kind":"gitlab","base_url":"https://gitlab.example.com","token":"glpat-secret"}""", bodies.last { it.isNotEmpty() })
        rule.onNodeWithText("Connect a forge").assertExists()
    }

    @Test
    fun theTokenFieldNamesTheScopeToUseForTheChosenForge() {
        val store = store()
        showSettings(store)

        rule.onNodeWithText("Connect a forge").performClick()
        rule.onNodeWithText("Use a token with the repo scope, or a fine-grained token with Contents, Issues and Pull requests permissions.").assertExists()

        rule.onNodeWithText("GitLab").performClick()
        rule.onNodeWithText("Use a token with the api scope. read_api is enough to browse only.").assertExists()
    }

    @Test
    fun theTokenFieldIsMaskedAndEmptyWhenTheFormOpensAgain() {
        val store = store()
        showSettings(store)

        rule.onNodeWithText("Connect a forge").performClick()
        rule.onNodeWithContentDescription("Personal access token").performTextInput("glpat-secret")

        rule.onNodeWithContentDescription("Personal access token").assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))

        rule.onNodeWithText("Connect").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("sam, slop").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Connect a forge").performClick()

        rule.onNodeWithContentDescription("Personal access token").assertTextEquals("")
    }

    @Test
    fun aServerErrorShowsItsMessageAndKeepsTheForm() {
        connectAnswer = HttpStatusCode.BadRequest to """{"error":"forge_token_rejected","message":"GitHub did not accept that token. Check it and try again."}"""
        val store = store()
        showSettings(store)

        rule.onNodeWithText("Connect a forge").performClick()
        rule.onNodeWithContentDescription("Personal access token").performTextInput("bad")
        rule.onNodeWithText("Connect").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("GitHub did not accept that token. Check it and try again.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Personal access token").assertExists()
        rule.onNodeWithText("Cancel").assertExists()
    }

    @Test
    fun cancellingClosesTheFormWithoutSendingAnything() {
        val store = store()
        showSettings(store)

        rule.onNodeWithText("Connect a forge").performClick()
        rule.onNodeWithText("Cancel").performClick()

        rule.onNodeWithText("Connect a forge").assertExists()
        assertEquals(0, requests.count { it.method == HttpMethod.Post })
    }

    @Test
    fun disconnectingRemovesTheRow() {
        val store = store()
        showSettings(store)

        rule.onNodeWithText("Disconnect").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("samexample").fetchSemanticsNodes().isEmpty() }
        assertEquals("/api/v1/forges/f1", requests.last { it.method == HttpMethod.Delete }.url.encodedPath)
    }
}
