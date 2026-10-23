package mycelium

import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.UsageWindow
import mycelium.state.accountChoices
import mycelium.state.usageClockTime
import mycelium.state.usageProjection
import mycelium.state.windowName
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.time.Instant

private val Now = Instant.parse("2026-10-23T14:00:00Z")

private fun utc(millis: Long): String = Instant.fromEpochMilliseconds(millis).toString().replace("Z", "").substring(0, 16)

class AccountUsageTest {
    @Test
    fun aTimeLaterTodayIsShownAsTheClockTimeAndAnotherDayWithItsDate() {
        assertEquals("22:30", usageClockTime("2026-10-23T22:30:00Z", ::utc, Now))
        assertEquals("28 Oct 09:00", usageClockTime("2026-10-28T09:00:00Z", ::utc, Now))
    }

    @Test
    fun withoutAKnownLocalTimeTheTimeIsShownAsHowLongAway() {
        assertEquals("in 8h", usageClockTime("2026-10-23T22:30:00Z", { null }, Now))
    }

    @Test
    fun aProjectionNamesWhenTheWindowRunsOutAndWhenItResets() {
        val window = UsageWindow("five_hour", 64.0, resetsAt = "2026-10-23T22:30:00Z", ratePerHour = 12.0, runsOutAt = "2026-10-23T18:40:00Z")

        assertEquals("At this rate, runs out about 18:40, resets 22:30", usageProjection(window, ::utc, Now))
    }

    @Test
    fun withoutAKnownLocalTimeTheProjectionSaysHowLongAway() {
        val window = UsageWindow("five_hour", 64.0, resetsAt = "2026-10-23T22:30:00Z", ratePerHour = 12.0, runsOutAt = "2026-10-23T18:40:00Z")

        assertEquals("At this rate, runs out in 4h, resets in 8h", usageProjection(window, { null }, Now))
    }

    @Test
    fun aWindowThatLastsUntilItResetsSaysSo() {
        val window = UsageWindow("seven_day", 20.0, resetsAt = "2026-10-28T09:00:00Z", ratePerHour = 0.2)

        assertEquals("At this rate, lasts until it resets, resets 28 Oct 09:00", usageProjection(window, ::utc, Now))
    }

    @Test
    fun aWindowWithNoRateOnlyGivesItsResetTime() {
        assertEquals("Resets 22:30", usageProjection(UsageWindow("five_hour", 5.0, resetsAt = "2026-10-23T22:30:00Z"), ::utc, Now))
        assertEquals("", usageProjection(UsageWindow("five_hour", 5.0), ::utc, Now))
    }

    @Test
    fun windowsAreNamedForPeople() {
        assertEquals(listOf("5-hour window", "weekly window", "weekly opus window"), listOf("five_hour", "seven_day", "seven_day_opus").map(::windowName))
    }

    @Test
    fun thePickerShowsTheWindowNearestItsLimit() {
        val login = LoginInfo("claude", "sam@example.com", "token", LoginState.SignedIn, usageLeft = 36, windows = listOf(UsageWindow("five_hour", 64.0), UsageWindow("seven_day", 91.0)))

        val choice = accountChoices(listOf(login), emptyList(), "willow").single()

        assertEquals("9% left, weekly window", choice.description)
        assertEquals(91, choice.usedPercent)
    }
}
