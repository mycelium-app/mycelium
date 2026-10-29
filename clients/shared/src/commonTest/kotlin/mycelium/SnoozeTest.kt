package mycelium

import mycelium.state.SnoozeChoice
import mycelium.state.customSnoozeUntil
import mycelium.state.snoozeLeft
import mycelium.state.snoozeUntil
import kotlin.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

private val Wednesday = Instant.parse("2026-10-28T20:30:00Z")
private val OneHourAhead = { millis: Long -> Instant.fromEpochMilliseconds(millis).plus(kotlin.time.Duration.parse("1h")).toString().take(16) }

class SnoozeTest {
    @Test
    fun inAnHourAndInThreeHoursAreRelativeToNow() {
        assertEquals(Instant.parse("2026-10-28T21:30:00Z"), snoozeUntil(SnoozeChoice.Hour, Wednesday, OneHourAhead))
        assertEquals(Instant.parse("2026-10-28T23:30:00Z"), snoozeUntil(SnoozeChoice.ThreeHours, Wednesday, OneHourAhead))
    }

    @Test
    fun tomorrowIsEightInTheMorningOnTheUsersClock() {
        assertEquals(Instant.parse("2026-10-29T07:00:00Z"), snoozeUntil(SnoozeChoice.Tomorrow, Wednesday, OneHourAhead))
    }

    @Test
    fun nextWeekIsTheNextMondayAtEightInTheMorning() {
        assertEquals(Instant.parse("2026-11-02T07:00:00Z"), snoozeUntil(SnoozeChoice.NextWeek, Wednesday, OneHourAhead))
        assertEquals(Instant.parse("2026-11-09T07:00:00Z"), snoozeUntil(SnoozeChoice.NextWeek, Instant.parse("2026-11-02T10:00:00Z"), OneHourAhead))
    }

    @Test
    fun aCustomTimeIsReadOnTheUsersClockAndMustBeInTheFuture() {
        assertEquals(Instant.parse("2026-10-30T08:15:00Z"), customSnoozeUntil("2026-10-30 09:15", Wednesday, OneHourAhead))
        assertNull(customSnoozeUntil("2026-10-28 09:15", Wednesday, OneHourAhead))
        assertNull(customSnoozeUntil("tomorrow", Wednesday, OneHourAhead))
    }

    @Test
    fun theTimeLeftUsesTheLargestWholeUnit() {
        assertEquals("14h left", snoozeLeft("2026-10-29T10:45:00Z", Wednesday))
        assertEquals("3d left", snoozeLeft("2026-11-01T10:00:00Z", Wednesday))
        assertEquals("20m left", snoozeLeft("2026-10-28T20:50:00Z", Wednesday))
    }
}
