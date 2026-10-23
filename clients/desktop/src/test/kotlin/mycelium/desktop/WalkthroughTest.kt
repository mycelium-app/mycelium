package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
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

private const val Walkthrough = """{"id":"w1","conversation_id":"c1","created_at":"2026-10-23T10:00:00Z","steps":[
{"title":"Add the shared strings file","key":false,"explanation":"A new file holds every screen's copy.","files":[{"path":"src/strings.ts","diff":"+export const strings = {}"}]},
{"title":"Restore reads its copy from strings","key":true,"explanation":"The change itself.","files":[{"path":"src/Screen.tsx","diff":"-const title = \"Restore\"\n+const title = strings.title"}]},
{"title":"Tests","key":false,"explanation":"","files":[]}]}"""

class WalkthroughTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show(walkthrough: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val reply = when {
                path.endsWith("/walkthrough") -> walkthrough
                path.endsWith("/goal") -> "null"
                path.endsWith("/nodes") -> "[]"
                path.endsWith("/conversations") -> "[$Chat]"
                path.endsWith("/conversations/c1") -> Chat
                path.endsWith("/events") || path.endsWith("/queue") -> "[]"
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, HandsOffPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    private fun openWalkthrough() {
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Walkthrough").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Walkthrough").performClick()
    }

    @Test
    fun aConversationWithoutAWalkthroughHasNoTabs() {
        show("null")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(0, rule.onAllNodesWithText("Walkthrough").fetchSemanticsNodes().size)
    }

    @Test
    fun onceAWalkthroughExistsTheHeaderOffersConversationAndWalkthroughTabs() {
        show(Walkthrough)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Walkthrough").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Conversation").assertIsDisplayed()
        rule.onNodeWithText("Walkthrough").assertIsDisplayed()
    }

    @Test
    fun theWalkthroughTabListsTheStepsInOrderWithTheKeyStepMarkedInWords() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithContentDescription("Step 1: Add the shared strings file").assertIsDisplayed()
        rule.onNodeWithContentDescription("Step 2: Restore reads its copy from strings, key step").assertIsDisplayed()
        rule.onNodeWithContentDescription("Step 3: Tests").assertIsDisplayed()
        rule.onNodeWithText("Key").assertIsDisplayed()
    }

    @Test
    fun theFirstStepShowsItsExplanationAndItsDiff() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithText("A new file holds every screen's copy.").assertIsDisplayed()
        rule.onNodeWithText("export const strings = {}").assertIsDisplayed()
        rule.onNodeWithText("Step 1 of 3").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Previous step").fetchSemanticsNodes().size)
    }

    @Test
    fun nextAndPreviousMoveBetweenStepsAndTheKeyStepSaysSo() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithText("Next step").performClick()

        rule.onNodeWithText("Step 2 of 3").assertIsDisplayed()
        rule.onNodeWithText("Key step").assertIsDisplayed()
        rule.onNodeWithText("const title = strings.title").assertIsDisplayed()
        rule.onNodeWithText("const title = \"Restore\"").assertIsDisplayed()

        rule.onNodeWithText("Previous step").performClick()
        rule.onNodeWithText("Step 1 of 3").assertIsDisplayed()
    }

    @Test
    fun choosingAStepFromTheListShowsIt() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithContentDescription("Step 3: Tests").performClick()

        rule.onNodeWithText("Step 3 of 3").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("Next step").fetchSemanticsNodes().size)
    }

    @Test
    fun theConversationTabBringsTheConversationBack() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithText("Conversation").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("Step 1 of 3").fetchSemanticsNodes().size)
    }
}
