package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
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

private const val LayoutJson = """{"files":[
{"path":"src/restore/Screen.tsx","staged":false,"unstaged":true,"untracked":false,"added":1,"removed":1,"rows":3,"hunks":[0],"fingerprint":"a"},
{"path":"src/restore/strings.ts","staged":false,"unstaged":true,"untracked":false,"added":2,"removed":0,"rows":3,"hunks":[0],"fingerprint":"b"}]}"""

private const val RowsJson = """{"ranges":[
{"path":"src/restore/Screen.tsx","from":0,"fingerprint":"a","rows":[{"k":"h","t":"@@ -1 +1 @@"},{"k":"d","o":1,"t":"const title = 1"},{"k":"a","n":1,"t":"const title = 2"}]},
{"path":"src/restore/strings.ts","from":0,"fingerprint":"b","rows":[{"k":"h","t":"@@ -1 +1,2 @@"},{"k":"a","n":1,"t":"const word = 3"},{"k":"a","n":2,"t":"const word = 4"}]}]}"""

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
                path.endsWith("/git/changes-layout") -> LayoutJson
                path.endsWith("/git/changes-rows") -> RowsJson
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
    fun openingAFileFromTheGitPanelOpensTheChangesTabInTheMainViewAndTheListStaysPut() {
        show()
        openGit()

        openFile("src/restore/Screen.tsx")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close Changes").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("const title = 2").assertIsDisplayed()
        rule.onNodeWithText("const word = 3").assertIsDisplayed()
        rule.onNodeWithContentDescription("Show the changes to src/restore/strings.ts").assertIsDisplayed()
        rule.onNodeWithText("Conversation").assertIsDisplayed()
    }

    @Test
    fun theFileShownInChangesIsMarkedInTheGitPanelList() {
        show()
        openGit()
        openFile("src/restore/Screen.tsx")

        rule.waitUntil(5_000) { rule.onAllNodes(hasContentDescription("Show the changes to src/restore/Screen.tsx") and SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Shown in Changes")).fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodes(hasContentDescription("Show the changes to src/restore/strings.ts") and SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Shown in Changes")).fetchSemanticsNodes().size)
    }

    @Test
    fun openingTheSameDiffAgainFocusesItsTabInsteadOfAddingAnother() {
        show()
        openGit()
        openFile("src/restore/Screen.tsx")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close Changes").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Conversation").performClick()
        assertEquals(0, rule.onAllNodesWithText("const title = 2").fetchSemanticsNodes().size)

        openFile("src/restore/Screen.tsx")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("const title = 2").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(1, rule.onAllNodesWithContentDescription("Close Changes").fetchSemanticsNodes().size)
    }

    @Test
    fun closingTheDiffTabReturnsToTheConversation() {
        show()
        openGit()
        openFile("src/restore/Screen.tsx")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close Changes").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Close Changes").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close Changes").fetchSemanticsNodes().isEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("Main view tabs").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("const title = 2").fetchSemanticsNodes().size)
    }

    @Test
    fun aMiddleClickOnATabClosesIt() {
        show()
        openGit()
        openFile("src/restore/Screen.tsx")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close Changes").fetchSemanticsNodes().isNotEmpty() }

        rule.onNode(hasText("Changes") and hasAnyAncestor(hasContentDescription("Main view tabs"))).performMouseInput { moveTo(center); press(MouseButton.Tertiary); release(MouseButton.Tertiary) }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Close Changes").fetchSemanticsNodes().isEmpty() }
    }
}
