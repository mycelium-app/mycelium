package mycelium

import mycelium.state.WhenKind
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
}
