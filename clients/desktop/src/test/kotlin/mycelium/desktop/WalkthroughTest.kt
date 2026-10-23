package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
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
import kotlin.test.assertTrue

private const val Chat = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/backups","title":"Backups","status":"done","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val Walkthrough = """{"id":"w1","conversation_id":"c1","created_at":"2026-10-23T10:00:00Z","steps":[
{"title":"Add the shared strings file","key":false,"section":"Strings","icon":"data","explanation":"A new file holds every screen's copy.","files":[{"path":"src/strings.ts","diff":"+export const strings = {}"}]},
{"title":"Restore reads its copy from strings","key":true,"section":"Strings","icon":"data","explanation":"The change itself.","files":[{"path":"src/Screen.tsx","diff":"-const title = \"Restore\"\n+const title = strings.title"}]},
{"title":"Tests","key":false,"section":"Checks","icon":"tests","explanation":"","files":[]},
{"title":"Loose end","key":false,"explanation":"","files":[]}]}"""

class WalkthroughTest {
    @get:Rule
    val rule = createComposeRule()

    private val requests = mutableListOf<Pair<String, String>>()
    private var reads = "[]"

    private fun show(walkthrough: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += "${request.method.value} $path" to String(request.body.toByteArray())
            val reply = when {
                path.endsWith("/reads") -> reads
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
    fun theWalkthroughTabListsTheStepsInOrderGroupedUnderSectionHeadingsWithTheKeyStepMarkedInWords() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithText("STRINGS").assertIsDisplayed()
        rule.onNodeWithText("CHECKS").assertIsDisplayed()
        rule.onNodeWithContentDescription("Step 1: Add the shared strings file", substring = true).assertIsDisplayed()
        rule.onNodeWithContentDescription("Step 2: Restore reads its copy from strings, key change", substring = true).assertIsDisplayed()
        rule.onNodeWithContentDescription("Step 3: Tests", substring = true).assertIsDisplayed()
        rule.onNodeWithText("Key").assertIsDisplayed()
    }

    @Test
    fun stepsWithoutASectionFormAnUnnamedGroupAtTheTop() {
        show(Walkthrough)
        openWalkthrough()

        val top = { description: String -> rule.onNodeWithContentDescription(description, substring = true).fetchSemanticsNode().positionInRoot.y }

        assertTrue(top("Step 4: Loose end") < top("Step 1: Add the shared strings file"))
        assertTrue(top("Step 1: Add the shared strings file") < top("Step 3: Tests"))
    }

    @Test
    fun theFirstStepShowsItsExplanationAndItsDiff() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithText("A new file holds every screen's copy.").assertIsDisplayed()
        rule.onNodeWithText("export const strings = {}").assertIsDisplayed()
        rule.onNodeWithText("Step 1 of 4").assertIsDisplayed()
    }

    @Test
    fun nextMovesOnAndPreviousGoesBackAndTheKeyChangeSaysSo() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithContentDescription("Next step").performClick()

        rule.onNodeWithText("Step 2 of 4").assertIsDisplayed()
        rule.onNodeWithText("Key change").assertIsDisplayed()
        rule.onNodeWithText("const title = strings.title").assertIsDisplayed()
        rule.onNodeWithText("const title = \"Restore\"").assertIsDisplayed()

        rule.onNodeWithContentDescription("Previous step").performClick()
        rule.onNodeWithText("Step 1 of 4").assertIsDisplayed()
    }

    @Test
    fun choosingAStepFromTheListShowsItAndNextOnTheLastStepStaysThere() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithContentDescription("Step 4: Loose end", substring = true).performClick()

        rule.onNodeWithText("Step 4 of 4").assertIsDisplayed()
        rule.onNodeWithContentDescription("Next step").performClick()
        rule.onNodeWithText("Step 4 of 4").assertIsDisplayed()
    }

    @Test
    fun aStepSelectedForAMomentShowsAsViewedAndIsSavedAsRead() {
        show(Walkthrough)
        openWalkthrough()
        assertEquals(0, rule.onAllNodesWithContentDescription("viewed", substring = true).fetchSemanticsNodes().size)

        rule.mainClock.advanceTimeBy(2_000)

        rule.onNodeWithContentDescription("Step 1: Add the shared strings file, viewed").assertIsDisplayed()
        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/walkthroughs/w1/reads/0" } }
    }

    @Test
    fun nextMarksTheStepItLeavesAsReadWithoutWaiting() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithContentDescription("Next step").performClick()

        rule.onNodeWithContentDescription("Step 1: Add the shared strings file, viewed").assertIsDisplayed()
        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/walkthroughs/w1/reads/0" } }
    }

    @Test
    fun stepsReadBeforeAreCheckedWhenTheTabOpens() {
        reads = "[2]"
        show(Walkthrough)
        openWalkthrough()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Step 3: Tests, viewed").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theDownArrowAndJMoveToTheNextStepAndTheUpArrowAndKGoBack() {
        show(Walkthrough)
        openWalkthrough()
        rule.onNodeWithContentDescription("Step 1: Add the shared strings file", substring = true).performClick()
        rule.onNodeWithContentDescription("Step 1: Add the shared strings file", substring = true).performKeyInput { pressKey(Key.DirectionDown) }
        rule.onNodeWithText("Step 2 of 4").assertIsDisplayed()

        rule.onNodeWithContentDescription("Step 2: Restore reads its copy from strings", substring = true).performKeyInput { pressKey(Key.J) }
        rule.onNodeWithText("Step 3 of 4").assertIsDisplayed()

        rule.onNodeWithContentDescription("Step 3: Tests", substring = true).performKeyInput { pressKey(Key.K) }
        rule.onNodeWithText("Step 2 of 4").assertIsDisplayed()

        rule.onNodeWithContentDescription("Step 2: Restore reads its copy from strings", substring = true).performKeyInput { pressKey(Key.DirectionUp) }
        rule.onNodeWithText("Step 1 of 4").assertIsDisplayed()
    }

    @Test
    fun theScopePickerOffersThreeScopesAndRegenerateQueuesAPromptForTheChosenOne() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithContentDescription("Walkthrough scope: This branch").performClick()
        rule.onNodeWithText("Uncommitted changes").assertIsDisplayed()
        rule.onNodeWithText("Last commit").assertIsDisplayed()
        rule.onNodeWithText("Last commit").performClick()
        rule.onNodeWithText("Regenerate").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "POST /api/v1/conversations/c1/prompt" } }
        val prompt = requests.first { it.first == "POST /api/v1/conversations/c1/prompt" }.second
        assertTrue(prompt.contains("the last commit") && prompt.contains("walkthrough tool"), prompt)
    }

    @Test
    fun thePickerShowsTheScopeLastChosenForTheConversationAfterLeavingTheTab() {
        show(Walkthrough)
        openWalkthrough()
        rule.onNodeWithContentDescription("Walkthrough scope: This branch").performClick()
        rule.onNodeWithText("Uncommitted changes").performClick()

        rule.onNodeWithText("Conversation").performClick()
        rule.onNodeWithText("Walkthrough").performClick()

        rule.onNodeWithContentDescription("Walkthrough scope: Uncommitted changes").assertIsDisplayed()
    }

    @Test
    fun theConversationTabBringsTheConversationBack() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithText("Conversation").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("Step 1 of 4").fetchSemanticsNodes().size)
    }

    @Test
    fun dismissingTheWalkthroughHidesItsTabAndSavesTheDismissal() {
        show(Walkthrough)
        openWalkthrough()

        rule.onNodeWithContentDescription("Dismiss walkthrough").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/walkthroughs/w1/dismissal" } }
        assertEquals(0, rule.onAllNodesWithText("Walkthrough").fetchSemanticsNodes().size)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aDismissedWalkthroughIsReopenedFromTheMoreMenu() {
        show(Walkthrough.replace("\"steps\"", "\"dismissed\":true,\"steps\""))
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Set a goal").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("Walkthrough").fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("More").performClick()
        rule.onNodeWithText("Show walkthrough").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "DELETE /api/v1/walkthroughs/w1/dismissal" } }
        rule.onNodeWithText("Walkthrough").assertIsDisplayed()
    }

    @Test
    fun aFileWithNoChangeInTheScopeSaysSoInsteadOfShowingZeroCounts() {
        show(Walkthrough.replace("\"files\":[]},\n{\"title\":\"Loose end\"", "\"files\":[{\"path\":\"src/untouched.ts\",\"diff\":\"\",\"unchanged\":true}]},\n{\"title\":\"Loose end\""))
        openWalkthrough()

        rule.onNodeWithContentDescription("Step 3: Tests", substring = true).performClick()

        rule.onNodeWithText("src/untouched.ts").assertIsDisplayed()
        rule.onNodeWithText("No changes to this file in this scope").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithText("+0", substring = true).fetchSemanticsNodes().size)
    }

    @Test
    fun aNoteAboutTheDiffsIsShownAboveTheStep() {
        show(Walkthrough.replace("\"steps\"", "\"note\":\"The changes in this scope could not be read from the node, so the diffs are as the agent wrote them.\",\"steps\""))
        openWalkthrough()

        rule.onNodeWithText("could not be read from the node", substring = true).assertIsDisplayed()
    }
}
