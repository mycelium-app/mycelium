package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test

private fun conversation(groupField: String) = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/home/claude/projects/app","title":"Imported","status":"done",
"acp_session_id":null,"mode":null,"model":null,$groupField"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val GroupsJson = """[{"id":"g1","name":"slop","folders":[{"id":"f1","prefix":"/home/claude/projects"}],"created_at":"2026-10-22T10:00:00Z"}]"""

class ConversationGroupTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show(groupField: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/groups") -> GroupsJson
                path.endsWith("/conversations") -> "[${conversation(groupField)}]"
                path.endsWith("/nodes") -> "[]"
                path.endsWith("/conversations/c1") -> conversation(groupField)
                path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); ConversationScreen(store, state, "c1", onArchived = {}) } }
        rule.onNodeWithContentDescription("Open details").performClick()
    }

    @Test
    fun theDetailsNameTheGroupAConversationIsInEvenWithoutAProject() {
        show(""""group_id":"g1",""")

        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithContentDescriptionCount("Group: slop") == 1 }
        rule.onNodeWithContentDescription("Group: slop").assertExists()
    }

    @Test
    fun theDetailsSayNoneWhenTheConversationIsInNoGroup() {
        show("")

        rule.onNodeWithContentDescription("Group: None").assertExists()
    }
}

private fun androidx.compose.ui.test.junit4.ComposeContentTestRule.onAllNodesWithContentDescriptionCount(description: String): Int =
    onAllNodesWithContentDescription(description).fetchSemanticsNodes().size
