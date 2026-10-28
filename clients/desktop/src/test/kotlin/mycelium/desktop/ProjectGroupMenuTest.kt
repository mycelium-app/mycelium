package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.onAllNodesWithText
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
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.ProjectsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val AppProject = """{"id":"p1","node_id":"n1","name":"homelab","path":"/p/homelab","group_id":"GROUP","created_at":"2026-10-22T10:00:00Z"}"""

private const val Groups = """[{"id":"g1","name":"slop","folders":[],"created_at":"2026-10-22T10:00:00Z"}]"""

class ProjectGroupMenuTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val patches = CopyOnWriteArrayList<Pair<String, String>>()

    private fun show(group: String?, width: Dp = 1024.dp) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val project = AppProject.replace("\"GROUP\"", group?.let { "\"$it\"" } ?: "null")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Patch) {
                val body = String(request.body.toByteArray())
                patches += path to body

                return@MockEngine respond(AppProject.replace("\"GROUP\"", if (body == "{}") "null" else "\"g1\""), HttpStatusCode.OK, json)
            }

            val body = when {
                path.endsWith("/groups") -> Groups
                path.endsWith("/projects") -> "[$project]"
                path.endsWith("/repositories") -> """{"repositories":[],"organisations":[],"failures":[]}"""
                path.endsWith("/forges") || path.endsWith("/nodes") || path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(width)) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } } }
    }

    @Test
    fun aProjectIsAssignedToAGroupFromItsRow() {
        show(null)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("homelab").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Group of homelab: No group").performClick()
        rule.onNodeWithText("slop").performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals(listOf("/api/v1/projects/p1" to """{"group_id":"g1"}"""), patches)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("slop").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aProjectIsUnassignedFromItsGroup() {
        show("g1")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("homelab").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Group of homelab: slop").performClick()
        rule.onNodeWithText("No group").performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals("{}", patches.single().second)
    }

    @Test
    fun theNewConversationButtonKeepsItsPaddingOnANarrowWindow() {
        show(null, width = 760.dp)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("homelab").fetchSemanticsNodes().isNotEmpty() }
        val button = rule.onNodeWithContentDescription("New conversation in homelab").fetchSemanticsNode().boundsInRoot
        val label = rule.onNodeWithText("New conversation", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot

        assertTrue(button.right - label.right >= 4f, "the label ends ${button.right - label.right}px from the button's edge")
    }
}
