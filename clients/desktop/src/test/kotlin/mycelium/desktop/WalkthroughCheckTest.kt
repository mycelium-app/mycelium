package mycelium.desktop

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.test.assertIsDisplayed
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
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
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

private const val Chat = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Strings","status":"done","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val WithChecks = """{"id":"w1","conversation_id":"c1","created_at":"2026-10-23T10:00:00Z","steps":[
{"title":"Add the shared strings file","key":false,"explanation":"A new file holds every screen's copy.","files":[{"path":"src/strings.ts","diff":"+export const strings = {}"}]},
{"title":"Restore reads its copy from strings","key":true,"explanation":"The change itself.","files":[{"path":"src/Screen.tsx","diff":"+const title = strings.title"}]}],
"checks":[{"step":2,"question":"Where does the title come from?","choices":["A literal","The strings file"]}]}"""

private val WithoutChecks = WithChecks.substringBefore(""",
"checks":""") + "}"

@OptIn(ExperimentalTestApi::class)
class WalkthroughCheckTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val answers = CopyOnWriteArrayList<String>()

    private fun show(walkthrough: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val reply = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.contains("/answers/") && request.method == HttpMethod.Put -> {
                    val body = String(request.body.toByteArray())
                    answers += body

                    if (body.contains("1")) {
                        """{"step":2,"choice":1,"correct":true,"explanation":"Step 2 reads it from strings."}"""
                    } else {
                        """{"step":2,"choice":0,"correct":false,"explanation":"Step 2 reads it from strings."}"""
                    }
                }
                path.endsWith("/answers") -> "[]"
                path.endsWith("/uncovered") || path.endsWith("/notes") -> "[]"
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
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, HandsOffPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Walkthrough").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Walkthrough").performClick()
    }

    private val question = "Where does the title come from?"

    private fun group(): SemanticsMatcher = SemanticsMatcher.keyIsDefined(SemanticsProperties.SelectableGroup) and hasContentDescription(question)

    private fun radio(label: String): SemanticsMatcher = SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.RadioButton) and hasText(label) and hasAnyAncestor(group())

    private fun checkButton(): SemanticsMatcher = SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button) and hasContentDescription("Check your answer to step 2")

    private fun openCheck() {
        rule.onNodeWithContentDescription("Step 3: Check your understanding", substring = true).performClick()
        rule.waitUntil(5_000) { rule.onAllNodes(group()).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aWalkthroughWithoutChecksHasNoCheckStep() {
        show(WithoutChecks)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Step 1: Add the shared strings file", substring = true).fetchSemanticsNodes().isNotEmpty() }

        assertEquals(0, rule.onAllNodesWithContentDescription("Step 3: Check your understanding", substring = true).fetchSemanticsNodes().size)
    }

    @Test
    fun eachQuestionIsARadioGroupWhoseWholeRowSelectsTheAnswer() {
        show(WithChecks)
        openCheck()

        rule.onNode(radio("The strings file")).assertIsNotSelected().performClick()

        rule.onNode(radio("The strings file")).assertIsSelected()
        rule.onNode(radio("A literal")).assertIsNotSelected()
    }

    @Test
    fun theRightChoiceReadsCorrectWithItsExplanationInALiveRegion() {
        show(WithChecks)
        openCheck()

        rule.onNode(radio("The strings file")).performClick()
        rule.onNode(checkButton()).performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Correct").fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(hasText("Correct") and hasAnyAncestor(SemanticsMatcher.keyIsDefined(SemanticsProperties.LiveRegion))).assertIsDisplayed()
        rule.onNodeWithText("Step 2 reads it from strings.").assertIsDisplayed()
        assertEquals(listOf("""{"choice":1}"""), answers.toList())
    }

    @Test
    fun aWrongChoiceReadsNotQuiteAndTheBackLinkReturnsToTheStep() {
        show(WithChecks)
        openCheck()

        rule.onNode(radio("A literal")).performClick()
        rule.onNode(checkButton()).performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Not quite").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Back to step 2").performClick()

        rule.onNodeWithText("The change itself.").assertIsDisplayed()
    }

    @Test
    fun checkingWithoutChoosingAsksForAnAnswerAndSendsNothing() {
        show(WithChecks)
        openCheck()

        rule.onNode(checkButton()).performClick()

        rule.onNodeWithText("Choose an answer to check it.").assertIsDisplayed()
        assertEquals(emptyList(), answers.toList())
    }

    @Test
    fun skippingRecordsNothingAndCanBeUndone() {
        show(WithChecks)
        openCheck()

        rule.onNode(hasText("Skip the questions") and hasClickAction()).performClick()

        assertEquals(emptyList(), answers.toList())
        rule.onNodeWithText("Answer the questions").performClick()
        rule.onNode(group()).assertExists()
    }
}
