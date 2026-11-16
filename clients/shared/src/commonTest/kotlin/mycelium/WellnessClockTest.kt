package mycelium

import mycelium.state.BreakClock
import mycelium.state.BreakPhase
import mycelium.state.DefaultWellnessSettings
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Minute = 60_000L

private fun BreakClock.after(minutes: Int, settings: mycelium.api.WellnessSettings, quiet: Boolean = false): BreakClock {
    val start = countedAt ?: 0L

    return (1..minutes).fold(copy(countedAt = start)) { clock, step -> clock.advance(settings, quiet, start + step * Minute, null) }
}

class WellnessClockTest {
    private val settings = DefaultWellnessSettings

    @Test
    fun aReminderIsDueAfterTheInterval() {
        assertEquals(BreakPhase.Idle, BreakClock().after(49, settings).phase)
        assertEquals(BreakPhase.Due, BreakClock().after(50, settings).phase)
    }

    @Test
    fun snoozingDelaysTheReminderByTheSnoozeLength() {
        val snoozed = BreakClock().after(50, settings).snooze(settings)

        assertEquals(BreakPhase.Snoozed(10 * Minute), snoozed.phase)
        assertEquals(BreakPhase.Snoozed(1 * Minute), snoozed.after(9, settings).phase)
        assertEquals(BreakPhase.Due, snoozed.after(10, settings).phase)
    }

    @Test
    fun aReminderCanBeSnoozedOnceAndThenOnlyEndsInABreak() {
        val back = BreakClock().after(50, settings).snooze(settings).after(10, settings)

        assertEquals(BreakPhase.Due, back.phase)
        assertEquals(60, back.activeMinutes)
        assertEquals(back, back.snooze(settings))
    }

    @Test
    fun takingABreakGivesTheNextReminderItsSnoozeBack() {
        val next = BreakClock().after(50, settings).snooze(settings).after(10, settings).startBreak().endBreak().backToWork().after(50, settings)

        assertEquals(BreakPhase.Snoozed(10 * Minute), next.snooze(settings).phase)
    }

    @Test
    fun switchingRemindersOffEndsAShownReminder() {
        val due = BreakClock().after(50, settings)

        assertEquals(BreakPhase.Idle, due.following(settings.copy(breakReminders = false)).phase)
        assertEquals(BreakPhase.Idle, due.snooze(settings).advance(settings.copy(breakReminders = false), false, due.countedAt!! + Minute, null).phase)
    }

    @Test
    fun noReminderComesWhileQuietHoursHold() {
        assertEquals(BreakPhase.Idle, BreakClock().after(120, settings, quiet = true).phase)
    }

    @Test
    fun noReminderComesWhenRemindersAreOff() {
        assertEquals(BreakPhase.Idle, BreakClock().after(120, settings.copy(breakReminders = false)).phase)
    }

    @Test
    fun startingABreakStopsTheClockUntilItEnds() {
        val onBreak = BreakClock().after(50, settings).startBreak()

        assertEquals(BreakPhase.OnBreak, onBreak.after(100, settings).phase)
        assertEquals(BreakPhase.Noting, onBreak.endBreak().phase)
        assertEquals(BreakPhase.Idle, onBreak.endBreak().backToWork().phase)
    }

    @Test
    fun theOverdueTimeKeepsCountingWhileTheReminderIsDue() {
        val due = BreakClock().after(50, settings)

        val later = due.advance(settings, false, due.countedAt!! + 754_000L, null)

        assertEquals(BreakPhase.Due, later.phase)
        assertEquals(754_000L, later.overdueMillis)
    }

    @Test
    fun workKeepsCountingForThreeMinutesAfterTheAppLosesFocusAndNoLonger() {
        val start = BreakClock(countedAt = 0)

        assertEquals(3 * Minute, start.advance(settings, false, 3 * Minute, awayFrom = 0).workedMillis)
        assertEquals(3 * Minute, start.advance(settings, false, 3 * Minute + 1_000, awayFrom = 0).workedMillis)
        assertEquals(3 * Minute, start.advance(settings, false, 40 * Minute, awayFrom = 0).workedMillis)
    }

    @Test
    fun aShortSwitchAwayCountsInFullAndWorkCarriesOnWhenTheAppIsBack() {
        val away = BreakClock(countedAt = 0).advance(settings, false, 2 * Minute, awayFrom = 0)

        val back = away.advance(settings, false, 5 * Minute, awayFrom = null)

        assertEquals(5 * Minute, back.workedMillis)
    }

    @Test
    fun aLongAbsencePausesTheClockWithoutClearingWhatWasWorked() {
        val worked = BreakClock().after(20, settings)
        val returned = worked.advance(settings, false, worked.countedAt!! + 30 * Minute, awayFrom = worked.countedAt)

        val resumed = returned.advance(settings, false, returned.countedAt!! + Minute, awayFrom = null)

        assertEquals(23 * Minute, returned.workedMillis)
        assertEquals(24 * Minute, resumed.workedMillis)
    }

    @Test
    fun oneLateCheckCountsTheWholeGapSoThrottledTimersLoseNothing() {
        val clock = BreakClock(countedAt = 0)

        assertEquals(BreakPhase.Due, clock.advance(settings, false, 52 * Minute, null).phase)
        assertEquals(2 * Minute, clock.advance(settings, false, 52 * Minute, null).overdueMillis)
    }

    @Test
    fun snoozingPausesTheOverdueTimeAndAStartedBreakResetsIt() {
        val due = BreakClock().after(55, settings)
        val snoozed = due.snooze(settings).after(4, settings)

        assertEquals(5 * Minute, snoozed.overdueMillis)
        assertEquals(0L, due.startBreak().overdueMillis)
        assertEquals(0L, due.startBreak().workedMillis)
    }
}
