package mycelium

import mycelium.api.Schedule
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.AppState
import mycelium.state.WhenKind
import mycelium.state.cronTime
import mycelium.state.cronWithTime
import mycelium.state.describeInWords
import mycelium.state.reduce
import mycelium.state.cronFor
import mycelium.state.describeCron
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class SchedulesStateTest {
    @Test
    fun cronExpressionsReadAsTheBoardShowsThem() {
        assertEquals("Mon 06:00", describeCron("0 6 * * 1"))
        assertEquals("Fri 17:00", describeCron("0 17 * * 5"))
        assertEquals("Sun 09:05", describeCron("5 9 * * 0"))
        assertEquals("Sun 09:05", describeCron("5 9 * * 7"))
        assertEquals("Daily 07:30", describeCron("30 7 * * *"))
        assertEquals("Every 15 minutes", describeCron("*/15 * * * *"))
        assertEquals("Every minute", describeCron("*/1 * * * *"))
        assertEquals("Every 6 hours", describeCron("0 */6 * * *"))
        assertEquals("Every hour", describeCron("0 */1 * * *"))
    }

    @Test
    fun anythingElseIsShownAsTheCronExpressionItself() {
        assertEquals("0 6 1 * *", describeCron("0 6 1 * *"))
        assertEquals("0 6-18 * * 1-5", describeCron("0 6-18 * * 1-5"))
        assertEquals("not cron", describeCron("not cron"))
    }

    @Test
    fun eachPresetBuildsItsCronExpression() {
        assertEquals("*/15 * * * *", cronFor(WhenKind.Every, 15, false, "", "Mon", ""))
        assertEquals("0 */2 * * *", cronFor(WhenKind.Every, 2, true, "", "Mon", ""))
        assertEquals("30 7 * * *", cronFor(WhenKind.Daily, 1, true, "07:30", "Mon", ""))
        assertEquals("0 17 * * 5", cronFor(WhenKind.Weekly, 1, true, "17:00", "Fri", ""))
        assertEquals("0 0 6 * *", cronFor(WhenKind.Cron, 1, true, "", "Mon", " 0 0 6 * * "))
    }

    @Test
    fun aPresetWithoutAUsableValueBuildsNothing() {
        assertNull(cronFor(WhenKind.Every, 0, true, "", "Mon", ""))
        assertNull(cronFor(WhenKind.Daily, 1, true, "7.30", "Mon", ""))
        assertNull(cronFor(WhenKind.Daily, 1, true, "24:00", "Mon", ""))
        assertNull(cronFor(WhenKind.Weekly, 1, true, "06:61", "Mon", ""))
        assertNull(cronFor(WhenKind.Cron, 1, true, "", "Mon", "  "))
    }

    @Test
    fun weekdaysReadInPlainWordsWithTheirTimeZone() {
        assertEquals("Weekdays 20:00", describeCron("0 20 * * 1-5"))
        assertEquals("Weekdays at 20:00, Europe/London", describeInWords("0 20 * * 1-5", "Europe/London"))
        assertEquals("Daily at 07:30, UTC", describeInWords("30 7 * * *", "UTC"))
        assertEquals("Every 15 minutes, UTC", describeInWords("*/15 * * * *", "UTC"))
    }

    @Test
    fun theTimeOfASimpleScheduleCanBeEditedWithoutTouchingItsDays() {
        assertEquals("20:00", cronTime("0 20 * * 1-5"))
        assertNull(cronTime("*/15 * * * *"))
        assertEquals("30 21 * * 1-5", cronWithTime("0 20 * * 1-5", "21:30"))
        assertNull(cronWithTime("0 20 * * 1-5", "9.30"))
    }

    private val proposed = """{"type":"schedule","id":"s1","node_id":"n1","agent":"claude","cwd":"/work/app","prompt":"Run it","cron":"0 20 * * 1-5","each_run":"same","paused":false,
"time_zone":"Europe/London","proposed":true,"conversation_id":"c1","next_run_at":"2099-01-01T20:00:00Z","last_run_at":null,"last_conversation_id":"c1","created_at":"2026-10-22T10:00:00Z"}"""

    @Test
    fun aStreamedScheduleIsStoredWithItsZoneAndProposalAndRemovedWhenDeclined() {
        val message = parseStreamMessage(proposed) as StreamMessage.ScheduleChanged
        val stored: Schedule = reduce(AppState(), message).schedules.getValue("s1")

        assertEquals(Triple("Europe/London", true, "c1"), Triple(stored.timeZone, stored.proposed, stored.conversationId))

        val removed = parseStreamMessage("""{"type":"schedule_removed","schedule_id":"s1"}""")!!

        assertEquals(emptyMap(), reduce(reduce(AppState(), message), removed).schedules)
    }
}
