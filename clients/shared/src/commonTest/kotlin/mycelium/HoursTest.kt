package mycelium

import mycelium.api.AlwaysOpenHours
import mycelium.api.Group
import mycelium.api.Hours
import mycelium.api.HoursDay
import mycelium.state.DefaultWellnessSettings
import mycelium.state.HoursAction
import mycelium.state.closedNotice
import mycelium.state.groupScope
import mycelium.state.hoursNotice
import mycelium.state.hoursSummary
import mycelium.state.hoursViewOf
import mycelium.state.inGroup
import mycelium.state.isOpen
import mycelium.state.minutesUntilChange
import mycelium.state.weekdayOf
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.time.Instant

private fun days(opens: String = "09:00", closes: String = "18:00", closedOn: Set<Int> = setOf(5, 6)) = (0..6).map { HoursDay(it, it in closedOn, opens, closes) }

private val Weekdays = Hours(alwaysOpen = false, days = days())

private fun group(id: String, name: String, hours: Hours = AlwaysOpenHours, openedUntil: String? = null) =
    Group(id, name, createdAt = "2026-10-01T09:00:00Z", hours = hours, openedUntil = openedUntil)

private val Utc = "2026-10-27T"

class HoursTest {
    @Test
    fun weekdaysCountFromMonday() {
        assertEquals(0, weekdayOf("2026-10-26"))
        assertEquals(6, weekdayOf("2026-11-01"))
    }

    @Test
    fun alwaysOpenHoursAreOpenAtAnyTime() {
        assertTrue(isOpen(AlwaysOpenHours, "2026-10-31T03:00"))
    }

    @Test
    fun weekdayHoursOpenInTheMorningCloseInTheEveningAndStayClosedAtTheWeekend() {
        assertTrue(isOpen(Weekdays, "2026-10-27T09:00"))
        assertTrue(isOpen(Weekdays, "2026-10-27T17:59"))
        assertFalse(isOpen(Weekdays, "2026-10-27T18:00"))
        assertFalse(isOpen(Weekdays, "2026-10-27T08:59"))
        assertFalse(isOpen(Weekdays, "2026-10-31T12:00"))
    }

    @Test
    fun aClosingTimeBeforeTheOpeningTimeRunsPastMidnight() {
        val night = Hours(alwaysOpen = false, days = days("22:00", "06:00", closedOn = emptySet()))

        assertTrue(isOpen(night, "2026-10-27T23:00"))
        assertTrue(isOpen(night, "2026-10-28T05:59"))
        assertFalse(isOpen(night, "2026-10-28T06:00"))
    }

    @Test
    fun twentyFourHundredCoversTheLastMinuteOfTheDay() {
        val late = Hours(alwaysOpen = false, days = days("09:00", "24:00"))

        assertTrue(isOpen(late, "2026-10-27T23:59"))
        assertFalse(isOpen(late, "2026-10-28T00:00"))
    }

    @Test
    fun theNextChangeIsTheClosingTimeWhileOpenAndTheOpeningTimeWhileClosed() {
        assertEquals(6 * 60, minutesUntilChange(listOf(Weekdays), "2026-10-27T12:00"))
        assertEquals(14 * 60, minutesUntilChange(listOf(Weekdays), "2026-10-27T19:00"))
        assertNull(minutesUntilChange(listOf(AlwaysOpenHours), "2026-10-27T12:00"))
    }

    @Test
    fun summariesNameTheCommonCases() {
        assertEquals("Always open", hoursSummary(AlwaysOpenHours))
        assertEquals("Weekdays 09:00 to 18:00", hoursSummary(Weekdays))
        assertEquals("Every day 09:00 to 18:00", hoursSummary(Hours(false, days(closedOn = emptySet()))))
        assertEquals("Mon, Wed 09:00 to 18:00", hoursSummary(Hours(false, days(closedOn = setOf(1, 3, 4, 5, 6)))))
        assertEquals("Set hours, 5 days", hoursSummary(Hours(false, days().mapIndexed { index, day -> if (index == 2) day.copy(closes = "17:00") else day })))
    }

    @Test
    fun aGroupIsClosedWhileMyceliumIsClosedAndAnOpeningOverridesBoth() {
        val settings = DefaultWellnessSettings.copy(hours = Weekdays)
        val now = Instant.parse("${Utc}19:00:00Z")
        val plain = group("w", "work")
        val opened = group("o", "slop", openedUntil = "${Utc}19:14:00Z")

        val view = hoursViewOf(settings, listOf(plain, opened), now, "${Utc}19:00")

        assertFalse(view.appOpen)
        assertTrue(view.closed("w"))
        assertFalse(view.closed("o"))
    }

    @Test
    fun anOpeningThatHasRunOutNoLongerKeepsTheGroupOpen() {
        val settings = DefaultWellnessSettings.copy(hours = Weekdays)
        val now = Instant.parse("${Utc}19:00:00Z")

        val view = hoursViewOf(settings, listOf(group("o", "slop", openedUntil = "${Utc}18:59:00Z")), now, "${Utc}19:00")

        assertTrue(view.closed("o"))
    }

    @Test
    fun theLastFifteenMinutesBeforeAGroupClosesShowACountdownAndOneSentenceForTheSameClosingTime() {
        val work = group("w", "work", Weekdays)
        val slop = group("s", "slop", Weekdays)
        val homelab = group("h", "homelab", AlwaysOpenHours)
        val now = Instant.parse("${Utc}17:48:00Z")
        val view = hoursViewOf(DefaultWellnessSettings, listOf(work, slop, homelab), now, "${Utc}17:48")

        val single = assertNotNull(hoursNotice(view, "w", null))
        val several = assertNotNull(hoursNotice(view, "all", null))

        assertEquals("Work closes at 18:00. Anything you send now keeps running overnight.", single.message)
        assertEquals(12, single.minutesLeft)
        assertEquals(HoursAction.OpenHours, single.action)
        assertEquals("Slop and Work close at 18:00.", several.message)
        assertNull(hoursNotice(view, "h", null))
    }

    @Test
    fun noCountdownShowsEarlierThanFifteenMinutesBeforeClosing() {
        val view = hoursViewOf(DefaultWellnessSettings, listOf(group("w", "work", Weekdays)), Instant.parse("${Utc}17:30:00Z"), "${Utc}17:30")

        assertNull(hoursNotice(view, "w", null))
    }

    @Test
    fun anOpeningOutOfHoursShowsWhenItEndsAndOffersToCloseItNow() {
        val view = hoursViewOf(DefaultWellnessSettings.copy(hours = Weekdays), listOf(group("w", "work", Weekdays, openedUntil = "${Utc}21:47:00Z")), Instant.parse("${Utc}21:33:00Z"), "${Utc}21:33")

        val notice = assertNotNull(hoursNotice(view, "w", null))

        assertEquals("Work is open out of hours until 21:47.", notice.message)
        assertEquals(14, notice.minutesLeft)
        assertEquals(HoursAction.CloseNow, notice.action)
    }

    @Test
    fun aClosedGroupSaysWhenItClosedWhenItOpensAndWhoElseIsOpen() {
        val view = hoursViewOf(DefaultWellnessSettings, listOf(group("w", "work", Weekdays), group("h", "homelab")), Instant.parse("${Utc}19:00:00Z"), "${Utc}19:00")

        val notice = assertNotNull(closedNotice(view, "w", 2, null))

        assertEquals("Work closed at 18:00", notice.title)
        assertEquals("It opens again tomorrow at 09:00. 2 agents are still working, and what they did will be here when it opens.", notice.body)
        assertEquals(listOf("homelab"), notice.openNow.map { it.name })
        assertEquals("Open Work for 15 minutes", notice.openLabel)
    }

    @Test
    fun myceliumBeingClosedSaysWhenEverythingOpens() {
        val view = hoursViewOf(DefaultWellnessSettings.copy(hours = Weekdays), listOf(group("w", "work")), Instant.parse("${Utc}19:00:00Z"), "${Utc}19:00")

        val notice = assertNotNull(closedNotice(view, "all", 3, null))

        assertEquals("Mycelium is closed until tomorrow 09:00", notice.title)
        assertEquals("3 agents are still working. Their results and notifications wait for the morning.", notice.body)
        assertEquals("Open a group for 15 minutes", notice.openLabel)
    }

    @Test
    fun anOpenGroupHasNoClosedScreen() {
        val view = hoursViewOf(DefaultWellnessSettings, listOf(group("h", "homelab")), Instant.parse("${Utc}19:00:00Z"), "${Utc}19:00")

        assertNull(closedNotice(view, "h", 0, null))
    }

    @Test
    fun allGroupsMeansTheOpenGroupsWhileAClosedOneIsLeftOut() {
        val scope = groupScope("all", setOf("w"))

        assertFalse(inGroup("w", scope))
        assertTrue(inGroup("h", scope))
        assertTrue(inGroup(null, scope))
        assertTrue(inGroup("w", groupScope("all", emptySet())))
    }
}
