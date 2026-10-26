package mycelium

import mycelium.state.DefaultWellnessSettings
import mycelium.state.quietAt
import mycelium.state.weekdayOf
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class QuietHoursTest {
    private val settings = DefaultWellnessSettings.copy(quietHours = true)

    @Test
    fun weekdaysCountFromMonday() {
        assertEquals(0, weekdayOf("2026-10-26"))
        assertEquals(6, weekdayOf("2026-11-01"))
    }

    @Test
    fun nothingIsQuietWhileQuietHoursAreOff() {
        assertFalse(quietAt(DefaultWellnessSettings, "2026-10-26T23:30"))
    }

    @Test
    fun aWindowSpanningMidnightEndsNextMorning() {
        assertTrue(quietAt(settings, "2026-10-26T23:30"))
        assertTrue(quietAt(settings, "2026-10-27T07:59"))
        assertFalse(quietAt(settings, "2026-10-27T08:00"))
        assertFalse(quietAt(settings, "2026-10-27T12:00"))
    }

    @Test
    fun weekendsAreQuietAllDay() {
        assertTrue(quietAt(settings, "2026-10-31T13:00"))
        assertTrue(quietAt(settings, "2026-11-01T13:00"))
    }

    @Test
    fun aDayThatIsSwitchedOffHoldsNothing() {
        val tuesdayOff = settings.copy(quietDays = settings.quietDays.map { if (it.weekday == 1) it.copy(enabled = false) else it })

        assertFalse(quietAt(tuesdayOff, "2026-10-27T23:30"))
        assertTrue(quietAt(tuesdayOff, "2026-10-27T03:00"))
    }
}
