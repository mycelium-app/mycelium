package mycelium.desktop

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.MouseButton
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performMouseInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Chat = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/backups","title":"Backups","status":"done","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val Status = """{"branch":"main","ahead":0,"behind":0,"files":[
{"path":"src/restore/Screen.tsx","staged":false,"unstaged":true,"untracked":false,"additions":1,"deletions":1},
{"path":"src/restore/strings.ts","staged":false,"unstaged":true,"untracked":false,"additions":2,"deletions":0}]}"""

private const val FileDiffJson = """{"diff":"@@ -1 +1 @@\n-const title = 1\n+const title = 2\n","truncated":false}"""

@OptIn(ExperimentalTestApi::class)
class DiffTabsTest {
    @get:Rule
    val rule = createComposeRule()

    private val requests = mutableListOf<String>()

    private fun show() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += path
            val reply = when {
                path.endsWith("/git/status") -> Status
                path.endsWith("/git/diff") -> FileDiffJson
                path.endsWith("/git/log") || path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") -> "[]"
                path.endsWith("/walkthrough") || path.endsWith("/goal") -> "null"
                path.endsWith("/conversations") -> "[$Chat]"
                path.endsWith("/conversations/c1") -> Chat
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }

        rule.setContent {
            MyceliumTheme(true) {
                Shell(store, HandsOffPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1"))
            }
        }
    }

    private fun openFile(path: String) {
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Show the changes to $path").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Show the changes to $path").performClick()
    }

    private fun openGit() {
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open git").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Open git").performClick()
    }

    @Test
    fun openingAFilesDiffFromTheGitPanelOpensATabInTheMainViewAndTheListStaysPut() {
        show()
        openGit()

        openFile("src/restore/Screen.tsx")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close diff of src/restore/Screen.tsx").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("const title = 2").assertIsDisplayed()
        rule.onNodeWithContentDescription("Show the changes to src/restore/strings.ts").assertIsDisplayed()
        rule.onNodeWithText("Conversation").assertIsDisplayed()
    }

    @Test
    fun openingTheSameDiffAgainFocusesItsTabInsteadOfAddingAnother() {
        show()
        openGit()
        openFile("src/restore/Screen.tsx")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close diff of src/restore/Screen.tsx").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Conversation").performClick()
        assertEquals(0, rule.onAllNodesWithText("const title = 2").fetchSemanticsNodes().size)

        openFile("src/restore/Screen.tsx")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("const title = 2").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(1, rule.onAllNodesWithContentDescription("Close diff of src/restore/Screen.tsx").fetchSemanticsNodes().size)
    }

    @Test
    fun closingTheDiffTabReturnsToTheConversation() {
        show()
        openGit()
        openFile("src/restore/Screen.tsx")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close diff of src/restore/Screen.tsx").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Close diff of src/restore/Screen.tsx").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close diff of src/restore/Screen.tsx").fetchSemanticsNodes().isEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("Main view tabs").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("const title = 2").fetchSemanticsNodes().size)
    }

    @Test
    fun aMiddleClickOnATabClosesIt() {
        show()
        openGit()
        openFile("src/restore/Screen.tsx")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close diff of src/restore/Screen.tsx").fetchSemanticsNodes().isNotEmpty() }

        rule.onNode(hasText("Screen.tsx") and hasAnyAncestor(hasContentDescription("Main view tabs"))).performMouseInput { moveTo(center); press(MouseButton.Tertiary); release(MouseButton.Tertiary) }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close diff of src/restore/Screen.tsx").fetchSemanticsNodes().isEmpty() }
    }
}
