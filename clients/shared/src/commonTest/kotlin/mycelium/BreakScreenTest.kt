package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.test.runTest
import mycelium.api.MyceliumApi
import mycelium.api.WellnessBreak
import mycelium.state.BreakPhase
import mycelium.state.WellnessStore
import mycelium.state.breakEndedCaption
import mycelium.state.breakWhen
import mycelium.state.countdownText
import mycelium.state.lengthText
import mycelium.state.minutesLeftText
import mycelium.state.stillWorkingOnBreak
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.time.Clock
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds

class BreakScreenTest {
    private fun store(scope: kotlinx.coroutines.CoroutineScope) =
        WellnessStore(MyceliumApi("https://mycelium.test", null, MockEngine { respond("", HttpStatusCode.NoContent) }), scope, null)

    @Test
    fun startingABreakBlocksTheAppUntilItsLengthHasPassed() = runTest {
        val wellness = store(backgroundScope)

        wellness.startBreak()

        assertEquals(BreakPhase.OnBreak, wellness.state.value.clock.phase)
        assertNotNull(wellness.state.value.breakEndsAt)

        wellness.finishBreakIfOver(Clock.System.now() + 1.minutes)

        assertEquals(BreakPhase.OnBreak, wellness.state.value.clock.phase)

        wellness.finishBreakIfOver(Clock.System.now() + 6.minutes)

        assertEquals(BreakPhase.Noting, wellness.state.value.clock.phase)
        assertNull(wellness.state.value.breakEndsAt)
        assertNull(wellness.state.value.ended?.earlyMinutes)
    }

    @Test
    fun endingABreakEarlyAsksWhatYouDidAndSaysHowEarly() = runTest {
        val wellness = store(backgroundScope)

        wellness.startBreak()
        wellness.endBreak()

        assertEquals(BreakPhase.Noting, wellness.state.value.clock.phase)
        assertNull(wellness.state.value.breakEndsAt)
        assertEquals(5, wellness.state.value.ended?.earlyMinutes)
    }

    @Test
    fun finishedAgentsAreCountedInTheCaptionOnlyWhenThereAreAny() {
        assertEquals("Your break ended at 14:35.", breakEndedCaption("14:35", null, 0))
        assertEquals("Your break ended at 14:35. 1 agent finished while you were away and its results are waiting.", breakEndedCaption("14:35", null, 1))
        assertEquals("Your break ended at 14:35. 2 agents finished while you were away and their results are waiting.", breakEndedCaption("14:35", null, 2))
        assertEquals("You ended your break 4 minutes early.", breakEndedCaption("14:31", 4, 2))
        assertEquals("You ended your break 1 minute early.", breakEndedCaption("14:34", 1, 0))
    }

    @Test
    fun recentBreaksSayWhenAndHowLongIncludingEarlyEnds() {
        assertEquals("Today, 14:30", breakWhen("2026-10-28T14:30", "2026-10-28"))
        assertEquals("Yesterday, 16:10", breakWhen("2026-10-27T16:10", "2026-10-28"))
        assertEquals("25 Oct, 09:00", breakWhen("2026-10-25T09:00", "2026-10-28"))
        assertEquals("5 min", WellnessBreak("b", "2026-10-28T14:30:00Z", "2026-10-28T14:35:00Z", plannedMinutes = 5).lengthText())
        assertEquals("1 min, ended early", WellnessBreak("b", "2026-10-28T11:05:00Z", "2026-10-28T11:10:00Z", plannedMinutes = 5, endedAt = "2026-10-28T11:06:00Z", endedEarly = true).lengthText())
        assertEquals("40 min", WellnessBreak("b", "2026-10-28T17:00:00Z", "2026-10-28T17:05:00Z", plannedMinutes = 5, endedAt = "2026-10-28T17:40:00Z").lengthText())
    }

    @Test
    fun theCountdownShowsMinutesAndPaddedSeconds() {
        assertEquals("4:12", countdownText(4.minutes + 12.seconds))
        assertEquals("0:05", countdownText(5.seconds))
        assertEquals("0:00", countdownText((-3).seconds))
    }

    @Test
    fun minutesLeftRoundUpAndAgreeInNumber() {
        assertEquals("1 minute", minutesLeftText(30.seconds))
        assertEquals("4 minutes", minutesLeftText(3.minutes + 20.seconds))
    }

    @Test
    fun runningWorkIsSaidToWaitUntilTheBreakEnds() {
        assertEquals("Results and notifications wait until the break ends.", stillWorkingOnBreak(0))
        assertEquals("1 agent is still working. Its results and notifications wait until the break ends.", stillWorkingOnBreak(1))
        assertEquals("2 agents are still working. Their results and notifications wait until the break ends.", stillWorkingOnBreak(2))
    }
}
