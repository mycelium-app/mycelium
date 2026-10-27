package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.test.runTest
import mycelium.api.MyceliumApi
import mycelium.state.BreakPhase
import mycelium.state.WellnessStore
import mycelium.state.countdownText
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

        assertEquals(BreakPhase.Idle, wellness.state.value.clock.phase)
        assertNull(wellness.state.value.breakEndsAt)
    }

    @Test
    fun endingABreakEarlyLiftsTheBreakScreen() = runTest {
        val wellness = store(backgroundScope)

        wellness.startBreak()
        wellness.endBreak()

        assertEquals(BreakPhase.Idle, wellness.state.value.clock.phase)
        assertNull(wellness.state.value.breakEndsAt)
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
