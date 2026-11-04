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
    fun aReminderCanBeSnoozedOnceAndThenOnlyEndsInABreak() {
        val back = BreakClock().after(50, settings).snooze(settings).after(10, settings)

        assertEquals(BreakPhase.Due, back.phase)
        assertEquals(60, back.activeMinutes)
        assertEquals(back, back.snooze(settings))
    }

    @Test
    fun takingABreakGivesTheNextReminderItsSnoozeBack() {
        val next = BreakClock().after(50, settings).snooze(settings).after(10, settings).startBreak().endBreak().backToWork().after(50, settings)

        assertEquals(BreakPhase.Snoozed(10), next.snooze(settings).phase)
    }

    @Test
    fun switchingRemindersOffEndsAShownReminder() {
        val due = BreakClock().after(50, settings)

        assertEquals(BreakPhase.Idle, due.following(settings.copy(breakReminders = false)).phase)
        assertEquals(BreakPhase.Idle, due.snooze(settings).tick(settings.copy(breakReminders = false), quiet = false).phase)
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
}
