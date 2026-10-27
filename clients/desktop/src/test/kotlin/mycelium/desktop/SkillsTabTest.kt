package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.WellnessScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

internal const val SkillsJson = """[
{"kind":"language","name":"Kotlin","own_lines":24,"agent_lines":76,"own_share":0.24,"weekly_own_lines":[0,2,3,5,4,8,12,24],"last_practised":"2026-10-26"},
{"kind":"language","name":"Rust","own_lines":12,"agent_lines":88,"own_share":0.12,"weekly_own_lines":[4,4,4,4,4,4,4,4],"last_practised":"2026-10-22"},
{"kind":"area","name":"clients/shared","own_lines":3,"agent_lines":97,"own_share":0.03,"weekly_own_lines":[0,0,0,0,0,0,1,2],"last_practised":null}]"""

private const val Suggested = """{"id":"e1","kind":"language","name":"Kotlin","title":"Debounce the sidebar group filter","brief":"Make the filter wait 150 ms.","minutes":20,"state":"suggested","reviewing":false,"created_at":"2026-10-23T10:00:00Z"}"""
private const val Started = """{"id":"e1","kind":"language","name":"Kotlin","title":"Debounce the sidebar group filter","brief":"Make the filter wait 150 ms.","minutes":20,"state":"started","branch":"learn/debounce-1","conversation_id":"c1","source_conversation_id":"c-src","source_title":"Usage card flicker","reviewing":false,"created_at":"2026-10-23T10:00:00Z","started_at":"2026-10-26T09:00:00Z"}"""
private const val Tests = """{"exit":1,"passed":5,"failed":1,"failing":["newGroupAppearsWithoutTyping"],"timed_out":false,"output_tail":""}"""
private const val Reviewed = """{"id":"e1","kind":"language","name":"Kotlin","title":"Debounce the sidebar group filter","brief":"Make the filter wait 150 ms.","minutes":20,"state":"reviewed","branch":"learn/debounce-1","conversation_id":"c1","reviewing":false,"tests":$Tests,"review":{"id":"r1","summary":"Close, one snapshot problem.","comments":[{"path":"Filter.kt","line":14,"body":"This reads groups once."},{"path":"Filter.kt","line":15,"body":"Good initial value.","praise":true}],"created_at":"2026-10-26T10:00:00Z"},"created_at":"2026-10-23T10:00:00Z","started_at":"2026-10-26T09:00:00Z"}"""

private const val KotlinRow = """{"kind":"language","name":"Kotlin","own_lines":24,"agent_lines":76,"own_share":0.24,"weekly_own_lines":[0,2,3,5,4,8,12,24],"last_practised":"2026-10-26"},"""
private const val HiddenKotlinJson = """[{"kind":"language","name":"Kotlin","own_lines":24,"agent_lines":76,"own_share":0.24,"weekly_own_lines":[0,2,3,5,4,8,12,24],"last_practised":"2026-10-26","hidden":true}]"""
private const val SolutionJson = """[{"path":"src/Filter.kt","diff":"@@ -1,2 +1,3 @@\n fun filter() {\n-    show()\n+    delay(150)\n+    show()\n }"}]"""
private const val LearnedJson = """[{"id":"l1","text":"combine versus zip for StateFlow","source_kind":"teach_me","conversation_id":"c1","conversation_title":"Usage card flicker","message_id":12,"created_at":"2026-10-27T09:00:00Z"}]"""

internal class LearningRequests {
    val seen = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())
    private var exercise = Suggested
    private var hidden = false

    fun sent(method: String, suffix: String) = seen.toList().any { it.first == method && it.second.endsWith(suffix) }

    fun api(): MyceliumApi {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            seen += request.method.value to path

            val reply = when {
                path.endsWith("/skills") && request.url.parameters["hidden"] == "true" -> if (hidden) HiddenKotlinJson else "[]"
                path.endsWith("/skills") -> if (hidden) SkillsJson.replace(KotlinRow, "") else SkillsJson
                path.endsWith("/skills/hidden") -> {
                    hidden = request.method.value == "PUT"

                    return@MockEngine respond("", HttpStatusCode.NoContent)
                }
                path.endsWith("/exercises/e1/solution") -> SolutionJson
                path.endsWith("/wellness/learned") -> LearnedJson
                path.endsWith("/exercises/suggested") -> if (exercise == Suggested) "[$Suggested]" else "[]"
                path.endsWith("/exercises") -> "[$exercise]"
                path.endsWith("/start") -> Started.also { exercise = it }
                path.endsWith("/tests") -> Tests.also { exercise = Started.replace("\"reviewing\":false", "\"reviewing\":false,\"tests\":$Tests") }
                path.endsWith("/review") -> Reviewed.also { exercise = it }
                path.endsWith("/wellness/overview") -> WellnessOverviewJson
                path.endsWith("/wellness/settings") -> WellnessSettingsJson
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }

        return MyceliumApi("https://mycelium.test", "token", engine)
    }
}

class SkillsTabTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = LearningRequests()

    private fun show() {
        val store = Store(requests.api(), scopes.create())

        runBlocking { store.wellness.load() }
        rule.setContent { MyceliumTheme(true) { WellnessScreen(store, OpeningPlatform()) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Suggested exercise").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun openSkills() {
        rule.onNodeWithText("Skills").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Rust").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theOverviewDrawsASparklinePerSkillAndOffersTheSuggestedExercise() {
        show()

        rule.onNodeWithContentDescription("Kotlin, lines you wrote each week, oldest first: 0, 2, 3, 5, 4, 8, 12, 24").assertExists()
        rule.onNodeWithContentDescription("Rust, lines you wrote each week", substring = true).assertExists()
        rule.onNodeWithText("Debounce the sidebar group filter").assertIsDisplayed()
    }

    @Test
    fun theSkillsTabListsSkillsByKindWithTheShareYouWrote() {
        show()
        openSkills()

        rule.onNodeWithText("Languages").assertIsDisplayed()
        rule.onNodeWithText("Areas of the codebase").assertIsDisplayed()
        rule.onNodeWithText("24%").assertIsDisplayed()
        rule.onNodeWithText("clients/shared").assertIsDisplayed()
    }

    @Test
    fun hidingASkillRemovesItFromTheList() {
        show()
        openSkills()

        rule.onNodeWithContentDescription("Hide Kotlin").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("24%").fetchSemanticsNodes().isEmpty() }
        assertTrue(requests.sent("PUT", "/skills/hidden"))
    }

    @Test
    fun aHiddenSkillIsListedUnderHiddenAndComesBackAfterUnhiding() {
        show()
        openSkills()

        rule.onNodeWithContentDescription("Hide Kotlin").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("24%").fetchSemanticsNodes().isEmpty() }

        rule.onNodeWithText("Hidden").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Unhide Kotlin").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("No exercises for a hidden skill. Unhide it to see it in the overview and get exercises for it again.").assertIsDisplayed()

        rule.onNodeWithContentDescription("Unhide Kotlin").performClick()
        rule.waitUntil(5_000) { requests.sent("DELETE", "/skills/hidden") }

        rule.onNodeWithText("All").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("24%").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun showASolutionOpensTheSourceDiffAndEscapeClosesIt() {
        show()
        rule.onNodeWithContentDescription("Start Debounce the sidebar group filter").performClick()
        rule.waitUntil(5_000) { requests.sent("POST", "/exercises/e1/start") }
        openSkills()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("learn/debounce-1").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Show a solution to Debounce the sidebar group filter").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("src/Filter.kt").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("A solution to Debounce the sidebar group filter").assertIsDisplayed()
        rule.onNodeWithText("What the agent wrote in Usage card flicker, the conversation this exercise came from. Looking is not recorded.").assertIsDisplayed()

        rule.onAllNodes(isRoot()).onLast().performKeyInput { pressKey(Key.Escape) }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("src/Filter.kt").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun openingTheConversationFromASolutionClosesItAndLoadsTheConversation() {
        show()
        rule.onNodeWithContentDescription("Start Debounce the sidebar group filter").performClick()
        rule.waitUntil(5_000) { requests.sent("POST", "/exercises/e1/start") }
        openSkills()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("learn/debounce-1").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Show a solution to Debounce the sidebar group filter").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("src/Filter.kt").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Open the conversation").performClick()

        rule.waitUntil(5_000) { requests.sent("GET", "/conversations/c-src/events") }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("src/Filter.kt").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun recentlyLearnedListsWhatTheAgentNoted() {
        show()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("combine versus zip for StateFlow").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Teach me in Usage card flicker").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun startingTheSuggestedExerciseThenRunningTestsAndAskingForReviewShowsTheResults() {
        show()

        rule.onNodeWithContentDescription("Start Debounce the sidebar group filter").performClick()
        rule.waitUntil(5_000) { requests.sent("POST", "/exercises/e1/start") }

        openSkills()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("learn/debounce-1").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Run tests for Debounce the sidebar group filter").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Tests: 5 passed, 1 failed (newGroupAppearsWithoutTyping).").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Ask for a review of Debounce the sidebar group filter").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Close, one snapshot problem.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("This reads groups once.").assertIsDisplayed()
        rule.onNodeWithText("Filter.kt:15, good").assertIsDisplayed()
    }
}
