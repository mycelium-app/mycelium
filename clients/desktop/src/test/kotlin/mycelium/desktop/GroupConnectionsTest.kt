package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
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
import mycelium.ui.ConversationConnectionsSettings
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private const val Yucca = """{"id":"g1","name":"yucca","folders":[],"created_at":"2026-10-22T10:00:00Z"}"""

private const val Immich = """{"id":"g2","name":"Immich","folders":[],"created_at":"2026-10-22T10:00:00Z"}"""

private const val Connected = """{"id":"gc1","source_group_id":"g1","target_group_id":"g2","source_name":"yucca","target_name":"Immich","created_at":"2026-10-24T10:00:00Z"}"""

class GroupConnectionsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val calls = java.util.Collections.synchronizedList(mutableListOf<String>())

    private fun show(experiments: String, groups: String = "[$Yucca,$Immich]", connections: String = "[]") {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val body = String(request.body.toByteArray())
            val path = request.url.encodedPath
            calls += "${request.method.value} $path $body"

            when {
                path.endsWith("/experiments") -> respond(experiments, HttpStatusCode.OK, json)
                path.endsWith("/groups") -> respond(groups, HttpStatusCode.OK, json)
                path.endsWith("/group-connections") && request.method == HttpMethod.Post -> respond(Connected, HttpStatusCode.Created, json)
                path.endsWith("/group-connections") -> respond(connections, HttpStatusCode.OK, json)
                path.contains("/group-connections/") -> respond(Connected.replace("\"created_at\"", "\"revoked_at\":\"2026-10-25T10:00:00Z\",\"created_at\""), HttpStatusCode.OK, json)
                else -> respond("[]", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refreshExperiments() }
        rule.setContent { MyceliumTheme(true) { ConversationConnectionsSettings(store) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("No conversations are connected.").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun connectingTwoGroupsSendsThePair() {
        show(EnabledExperiments)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connect groups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Connect groups").performClick()
        rule.onNodeWithText("Connect").performClick()
        rule.waitUntil(5_000) { calls.any { it.startsWith("POST /api/v1/group-connections") } }

        assertTrue(calls.any { it.startsWith("POST /api/v1/group-connections") && it.contains("\"source_group_id\":\"g2\"") && it.contains("\"target_group_id\":\"g1\"") })
        rule.waitUntil(5_000) { rule.onAllNodesWithText("No groups are connected.").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun choosingTheSameGroupTwiceSaysToChooseTwoDifferentGroups() {
        show(EnabledExperiments)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Connect groups").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Connect groups").performClick()
        rule.onNodeWithContentDescription("Second group: yucca").performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.OnClick)
        rule.waitForIdle()
        val immich = rule.onAllNodesWithText("Immich")
        immich[immich.fetchSemanticsNodes().size - 1].performClick()
        rule.onNodeWithText("Connect").performClick()

        rule.onNodeWithText("Choose two different groups.").assertExists()
        assertTrue(calls.none { it.startsWith("POST /api/v1/group-connections") })
    }

    @Test
    fun aGroupConnectionIsRevokedFromItsRowMenu() {
        show(EnabledExperiments, connections = "[$Connected]")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Immich").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for yucca and Immich").performClick()
        rule.onNodeWithText("Revoke").performClick()
        rule.waitUntil(5_000) { calls.any { it.startsWith("DELETE /api/v1/group-connections/gc1") } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("No groups are connected.").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun withOneGroupItSaysToCreateASecondGroup() {
        show(EnabledExperiments, groups = "[$Yucca]")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Create a second group in Settings, Groups to connect groups.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Connect groups").assertDoesNotExist()
    }
}
