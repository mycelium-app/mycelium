package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.ProjectsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val AppProject = """{"id":"p1","node_id":"n1","name":"homelab","path":"/p/homelab",ICON"created_at":"2026-10-22T10:00:00Z"}"""

class ProjectIconsTest {
    @get:Rule
    val rule = createComposeRule()

    private val patches = mutableListOf<Pair<String, String>>()

    private fun show(icon: String? = null) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Patch) {
                patches += path to String(request.body.toByteArray())

                return@MockEngine respond(AppProject.replace("ICON", "\"icon\":\"rocket\","), HttpStatusCode.OK, json)
            }

            val body = when {
                path.endsWith("/projects") -> "[${AppProject.replace("ICON", icon?.let { "\"icon\":\"$it\"," }.orEmpty())}]"
                path.endsWith("/repositories") -> """{"repositories":[],"organisations":[],"failures":[]}"""
                path.endsWith("/forges") || path.endsWith("/nodes") || path.endsWith("/conversations") || path.endsWith("/groups") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ProjectsScreen(store, state, onNewConversation = {}) } }
    }

    @Test
    fun anIconIsFoundBySearchPickedAndSavedFromTheProjectMenu() {
        show()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("homelab").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Options for homelab").performClick()
        rule.onNodeWithText("Change icon…").performClick()
        rule.onNodeWithText("PNG, JPEG or WebP").assertExists()
        rule.onNodeWithContentDescription("Search icons").performTextInput("rock")
        rule.onAllNodesWithContentDescription("server").fetchSemanticsNodes().let { assertEquals(0, it.size) }
        rule.onNodeWithContentDescription("rocket").performClick()
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals(listOf("/api/v1/projects/p1" to """{"icon":"rocket"}"""), patches)
    }

    @Test
    fun noIconSendsAClearRequestForAProjectThatHasOne() {
        show("rocket")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("homelab").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Options for homelab").performClick()
        rule.onNodeWithText("Change icon…").performClick()
        rule.onNodeWithText("No icon").performClick()
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { patches.isNotEmpty() }
        assertEquals(listOf("/api/v1/projects/p1" to """{"clear_icon":true}"""), patches)
    }
}
