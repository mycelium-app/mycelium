package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.headersOf
import java.util.concurrent.atomic.AtomicReference
import mycelium.api.MyceliumApi
import mycelium.api.PullRequestItem
import mycelium.api.PullRequestState
import mycelium.api.StreamMessage
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.PullRequestPanel
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val RequestJson = """{"forge_id":"f1","repository":"example/harbour","number":9,"label":"PR #9","title":"Restore","url":"https://example.org/pull/9","state":"open","body":"Original description"}"""

class PullRequestPanelTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    @Test
    fun unresolvedCommentsStayVisibleAndForgeChangesRefreshTheOpenTimeline() {
        val comment = AtomicReference("Check the retry")
        val engine = MockEngine {
            respond(
                """{"pull_request":$RequestJson,"timeline":[{"body":"${comment.get()}","resolved":false},{"body":"Already fixed","resolved":true}]}""",
                headers = headersOf(HttpHeaders.ContentType, "application/json"),
            )
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        val request = PullRequestItem("f1", "example/harbour", 9, "PR #9", "Restore", "https://example.org/pull/9", state = PullRequestState.Open)
        rule.setContent { MyceliumTheme(true) { PullRequestPanel(store, request) } }
        rule.onNodeWithText("Timeline").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Already fixed").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Show unresolved comments only").performClick()
        rule.onNodeWithText("Check the retry").assertExists()
        assertEquals(0, rule.onAllNodesWithText("Already fixed").fetchSemanticsNodes().size)
        comment.set("Check the new error path")
        store.receive(StreamMessage.ForgeChanged)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Check the new error path").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("Already fixed").fetchSemanticsNodes().size)
    }

    @Test
    fun aForgeChangeRefreshesTheDescriptionWithoutClosingTheSummary() {
        val description = AtomicReference("Original description")
        val engine = MockEngine {
            respond(
                """{"pull_request":${RequestJson.replace("Original description", description.get())},"timeline":[]}""",
                headers = headersOf(HttpHeaders.ContentType, "application/json"),
            )
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        val request = PullRequestItem("f1", "example/harbour", 9, "PR #9", "Restore", "https://example.org/pull/9", state = PullRequestState.Open)
        rule.setContent { MyceliumTheme(true) { PullRequestPanel(store, request) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Original description").fetchSemanticsNodes().isNotEmpty() }

        description.set("Updated description")
        store.receive(StreamMessage.ForgeChanged)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Updated description").fetchSemanticsNodes().isNotEmpty() }
    }
}
