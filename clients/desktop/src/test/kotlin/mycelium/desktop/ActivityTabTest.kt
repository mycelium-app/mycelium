package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.hasContentDescription
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.time.Instant
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import mycelium.state.ActivityKind
import mycelium.state.ActivityTarget
import mycelium.state.Store
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.WellnessScreen
import org.junit.Rule

class ActivityTabTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = WellnessRequests()
    private val platform = OpeningPlatform()
    private val opened = mutableListOf<String>()

    private fun show(touch: Boolean = false): Store {
        val store = Store(requests.api(), scopes.create())

        rule.setContent {
            CompositionLocalProvider(LocalTouch provides touch) {
                MyceliumTheme(true) { WellnessScreen(store, platform, startOn = "activity", onOpenConversation = { opened += it }) }
            }
        }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Wednesday 28 October").fetchSemanticsNodes().isNotEmpty() }

        return store
    }

    private fun buttonNamed(name: String) = hasContentDescription(name) and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Button)

    @Test
    fun todayShowsTheTotalAndHidesNextDay() {
        show()

        rule.onNodeWithText("3 h 45 min on screen").assertExists()
        rule.onAllNodesWithContentDescription("Next day", substring = true).assertCountEquals(0)
        rule.onNodeWithContentDescription("Previous day, Tuesday 27 October").assertExists()
    }

    @Test
    fun steppingBackShowsTheEarlierDayWithTodayAndNextDayControls() {
        show()

        rule.onNodeWithContentDescription("Previous day, Tuesday 27 October").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Tuesday 27 October").fetchSemanticsNodes().isNotEmpty() }

        assertTrue(requests.queries.any { it.endsWith("/wellness/activity/day?date=2026-10-27") })
        rule.onNodeWithContentDescription("Next day, Wednesday 28 October").assertExists()
        rule.onNodeWithText("Today").assertExists()

        rule.onNodeWithContentDescription("Next day, Wednesday 28 October").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("3 h 45 min on screen").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun todayReturnsFromAnEarlierDay() {
        show()

        rule.onNodeWithContentDescription("Previous day, Tuesday 27 October").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Today").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Today").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("3 h 45 min on screen").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun anEmptyDayAfterRecordingBeganSaysMyceliumWasNotOpen() {
        show()

        rule.onNodeWithContentDescription("Previous day, Tuesday 27 October").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Nothing on screen on Tuesday 27 October").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Mycelium was not open that day.", substring = true).assertExists()
    }

    @Test
    fun aDayBeforeRecordingBeganSaysWhenRecordingBegan() {
        show()

        rule.onNodeWithContentDescription("Wednesday 22:00 to 23:00, nothing").assertExists()
        rule.onNodeWithContentDescription("Thursday 22:00 to 23:00, nothing").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Nothing on screen on Thursday 22 October").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Mycelium began recording the time of day on Saturday 24 October.", substring = true).assertExists()
    }

    @Test
    fun everyStretchIsAButtonNamedWithWhatKindGroupAndTimes() {
        show()

        rule.onNode(buttonNamed("harbour Bump the UI kit to 0.31.2. Conversation in homelab. 10:05 to 11:30, 1 h 25 min")).assertExists()
        rule.onNode(buttonNamed("Weekend ferry times. Chat in personal. 09:40 to 10:05, 25 min")).assertExists()
        rule.onNode(buttonNamed("Archivist. Assistant in homelab. 11:36 to 12:01, 25 min")).assertExists()
        rule.onNode(buttonNamed("History. Elsewhere in Mycelium. 08:52 to 09:05, 13 min")).assertExists()
        rule.onNode(buttonNamed("Break, 11:30 to 11:34, 4 min")).assertExists()
    }

    @Test
    fun rightArrowMovesFocusToTheNextStretch() {
        show()

        val first = "atlas Release v0.3.7 checklist. Conversation in work. 08:14 to 08:52, 38 min"

        rule.onNodeWithContentDescription(first).performSemanticsAction(SemanticsActions.RequestFocus)
        rule.onNodeWithContentDescription(first).performKeyInput { pressKey(Key.DirectionRight) }
        rule.waitForIdle()

        rule.onNodeWithContentDescription("History. Elsewhere in Mycelium. 08:52 to 09:05, 13 min").assertIsFocused()
    }

    @Test
    fun theHeatmapCellsAreNamedByDayHourAndMinutes() {
        show()

        rule.onNode(buttonNamed("Wednesday 10:00 to 11:00, 42 minutes")).assertExists()
        rule.onNode(buttonNamed("Wednesday 11:00 to 12:00, 5 minutes")).assertExists()
        rule.onNode(buttonNamed("Tuesday 14:00 to 15:00, nothing")).assertExists()
    }

    @Test
    fun clickingAHeatmapCellShowsThatDayAndTheHeatmapKeepsAllSevenDays() {
        show()

        rule.onNodeWithContentDescription("Tuesday 14:00 to 15:00, nothing").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Nothing on screen on Tuesday 27 October").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Wednesday 10:00 to 11:00, 42 minutes").assertExists()
        rule.onNodeWithContentDescription("Wednesday, shown in the timeline", useUnmergedTree = true).assertDoesNotExist()
        rule.onNodeWithContentDescription("Tuesday, shown in the timeline", useUnmergedTree = true).assertExists()
    }

    @Test
    fun theTableListsWhatWasOnScreenAndMergesElsewhereIntoOneRow() {
        show()

        rule.onNodeWithText("What was on screen").assertExists()
        rule.onNodeWithText("History, Wellness & Learning").assertExists()
        rule.onNodeWithContentDescription("Open harbour Bump the UI kit to 0.31.2").performClick()

        assertEquals(listOf("c-harbour"), opened)
    }

    @Test
    fun onAPhoneTappingAStretchFillsTheDetailsAndTappingACellOffersToShowItsDay() {
        show(touch = true)

        rule.onNodeWithContentDescription("harbour Bump the UI kit to 0.31.2. Conversation in homelab. 10:05 to 11:30, 1 h 25 min").performClick()
        rule.onNodeWithText("10:05 to 11:30, 1 h 25 min").assertExists()

        rule.onNodeWithContentDescription("Wednesday 10:00 to 11:00, 42 minutes").performClick()
        rule.onNodeWithText("Mostly harbour Bump the UI kit to 0.31.2").assertExists()
        rule.onNodeWithText("Show this day").assertExists()
    }

    @Test
    fun theHeartbeatNamesWhatIsOnScreen() {
        val store = Store(requests.api(), scopes.create())

        store.wellness.presence = ActivityTarget(ActivityKind.Conversation, "conversation", "Bump the UI kit", conversationId = "c-harbour")
        store.wellness.heartbeat(60, Instant.parse("2026-10-28T10:06:00Z"))

        runBlocking {
            withTimeout(5_000) {
                while (requests.sent("POST", "/wellness/heartbeat") == null) {
                    delay(10)
                }
            }
        }

        val body = requests.sent("POST", "/wellness/heartbeat")!!.third

        assertTrue(body.contains("\"kind\":\"conversation\"") && body.contains("\"conversation_id\":\"c-harbour\"") && body.contains("\"at\":\"2026-10-28T10:06:00Z\""), body)
    }
}
