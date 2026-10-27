package mycelium.desktop

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.serialization.json.Json
import mycelium.api.ChatToolServer
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.AccountPage
import mycelium.ui.IntegrationCategory
import mycelium.ui.Integration
import mycelium.ui.MyceliumTheme
import mycelium.ui.PageActions
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val DocsJson = """{"id":"s1","name":"Docs","kind":"mcp","url":"https://docs.example.com/mcp","settings":{"app":"outline"},"token_hint":"cdef","enabled":true,"tools":[{"name":"search","description":"Search","kind":"read"},{"name":"create_doc","description":"Create","kind":"write","asks":true}],"created_at":"2026-10-27T10:00:00Z","token_expires_at":"2026-11-26T00:00:00Z","status":"expires_soon"}"""

private const val GrantJson = """[{"token_id":"t1","conversation_id":"c1","conversation_title":"Upload the transcripts","scopes":["read"],"expires_at":"2026-10-27T18:00:00Z","calls":3}]"""

class AccountPageTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Triple<HttpMethod, String, String>>()

    private fun open() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += Triple(request.method, path, String(request.body.toByteArray()))

            when {
                request.method == HttpMethod.Patch -> respond(DocsJson, HttpStatusCode.OK, json)
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/api-access") -> respond(GrantJson, HttpStatusCode.OK, json)
                else -> respond("[]", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        val account = Json { ignoreUnknownKeys = true }.decodeFromString(ChatToolServer.serializer(), DocsJson)
        val integration = Integration("outline", "Outline", "mcp", IntegrationCategory.Docs, listOf(account), null)
        val actions = PageActions({}, {}, {}, {}, {}, {}, {})

        rule.setContent { MyceliumTheme(true) { AccountPage(store, integration, account, actions, {}, {}) } }
    }

    @Test
    fun theOverviewShowsTheExpiryBesideTheTokenAndEveryChangeIsADialogWithSave() {
        open()

        rule.onAllNodesWithText("26 Nov 2026", substring = true).onFirst().assertExists()
        rule.onNodeWithText("Change expiry").performClick()
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == HttpMethod.Patch } }
        assertEquals("""{"clear_token_expiry":true}""", requests.first { it.first == HttpMethod.Patch }.third)
    }

    @Test
    fun cancellingTheDialogChangesNothing() {
        open()

        rule.onNodeWithText("Change expiry").performClick()
        rule.onNodeWithText("Cancel").performClick()

        rule.onAllNodesWithText("Save").assertCountEquals(0)
        assertTrue(requests.none { it.first == HttpMethod.Patch })
    }

    @Test
    fun aLiveApiGrantIsListedAndRevokingEndsItsSession() {
        open()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Upload the transcripts").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Revoke").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == HttpMethod.Delete } }
        assertEquals("/api/v1/sessions/t1", requests.first { it.first == HttpMethod.Delete }.second)
    }
}
