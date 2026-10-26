package mycelium

import mycelium.state.BreakClock
import mycelium.state.BreakPhase
import mycelium.state.DefaultWellnessSettings
import kotlin.test.Test
import kotlin.test.assertEquals

private fun BreakClock.after(minutes: Int, settings: mycelium.api.WellnessSettings, quiet: Boolean = false) = (1..minutes).fold(this) { clock, _ -> clock.tick(settings, quiet) }

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

        assertEquals(BreakPhase.Snoozed(10), snoozed.phase)
        assertEquals(BreakPhase.Snoozed(1), snoozed.after(9, settings).phase)
        assertEquals(BreakPhase.Due, snoozed.after(10, settings).phase)
    }

    @Test
    fun dismissingStartsTheFullIntervalAgain() {
        val dismissed = BreakClock().after(50, settings).dismiss()

        assertEquals(BreakPhase.Idle, dismissed.after(49, settings).phase)
        assertEquals(BreakPhase.Due, dismissed.after(50, settings).phase)
    }

    @Test
    fun noReminderComesWhileQuietHoursHold() {
        assertEquals(BreakPhase.Idle, BreakClock().after(120, settings, quiet = true).phase)
    }

    @Test
    fun noReminderComesWhenRemindersAreOffAndPomodoroIsOff() {
        assertEquals(BreakPhase.Idle, BreakClock().after(120, settings.copy(breakReminders = false)).phase)
    }

    @Test
    fun pomodoroRemindsAfterTwentyFiveMinutesInsteadOfTheInterval() {
        val pomodoro = settings.copy(pomodoro = true, breakReminders = false)

        assertEquals(BreakPhase.Due, BreakClock().after(25, pomodoro).phase)
    }

    @Test
    fun pomodoroTakesALongBreakAfterTheFourthBlock() {
        val pomodoro = settings.copy(pomodoro = true)
        val lengths = (1..4).runningFold(BreakClock()) { clock, _ -> clock.startBreak().endBreak() }.dropLast(1).map { it.breakLength(pomodoro) }

        assertEquals(listOf(5, 5, 5, 15), lengths)
    }

    @Test
    fun startingABreakStopsTheClockUntilItEnds() {
        val onBreak = BreakClock().after(50, settings).startBreak()

        assertEquals(BreakPhase.OnBreak, onBreak.after(100, settings).phase)
        assertEquals(BreakPhase.Idle, onBreak.endBreak().phase)
    }
}
