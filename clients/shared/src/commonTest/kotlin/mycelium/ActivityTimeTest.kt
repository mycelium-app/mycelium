package mycelium

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import mycelium.api.ActivitySpan
import mycelium.state.ActivityKind
import mycelium.state.activityRows
import mycelium.state.addDays
import mycelium.state.dayHeading
import mycelium.state.heatLevel
import mycelium.state.heatName
import mycelium.state.rangeOf
import mycelium.state.stretchOf

private fun span(kind: String, title: String, start: String, end: String, conversation: String? = null, project: String? = null) =
    ActivitySpan("2026-10-28T${start}:00Z", "2026-10-28T${end}:00Z", kind, title, project = project, conversationId = conversation)

class ActivityTimeTest {
    @Test
    fun steppingADayCrossesMonthAndYearEdges() {
        assertEquals("2026-10-27", addDays("2026-10-28", -1))
        assertEquals("2026-11-01", addDays("2026-10-31", 1))
        assertEquals("2025-12-31", addDays("2026-01-01", -1))
    }

    @Test
    fun theDayHeadingNamesTheWeekdayAndMonth() {
        assertEquals("Wednesday 28 October", dayHeading("2026-10-28"))
        assertEquals("Sunday 1 November", dayHeading("2026-11-01"))
    }

    @Test
    fun heatStepsChangeAtFifteenThirtyAndFortyFiveMinutes() {
        assertEquals(listOf(0, 1, 1, 2, 2, 3, 3, 4, 4), listOf(0L, 1L, 14L, 15L, 29L, 30L, 44L, 45L, 60L).map(::heatLevel))
    }

    @Test
    fun aHeatCellIsNamedByWeekdayHourAndMinutes() {
        assertEquals("Wednesday 14:00 to 15:00, 42 minutes", heatName("2026-10-28", 14, 42))
        assertEquals("Wednesday 14:00 to 15:00, 1 minute", heatName("2026-10-28", 14, 1))
        assertEquals("Wednesday 23:00 to 00:00, nothing", heatName("2026-10-28", 23, 0))
    }

    @Test
    fun theTimelineRunsFromTheFirstToTheLastActivityRoundedToWholeHours() {
        val stretches = listOf(span("conversation", "a", "08:14", "08:52"), span("assistant", "b", "14:50", "14:57")).mapNotNull { stretchOf(it, null) }
        val range = rangeOf(stretches)

        assertEquals(8 to 15, range?.let { it.startHour to it.endHour })
        assertNull(rangeOf(emptyList()))
    }

    @Test
    fun aStretchKnowsItsKindAndLength() {
        val stretch = stretchOf(span("chat", "Ferry", "09:40", "10:05"), null)

        assertEquals(ActivityKind.Chat to 25, stretch?.let { it.kind to it.minutes })
    }

    @Test
    fun theTableMergesRepeatVisitsAndFoldsElsewhereIntoOneRow() {
        val rows = activityRows(
            listOf(
                span("conversation", "Bump", "10:00", "10:30", "c1", "harbour"),
                span("elsewhere", "History", "10:30", "10:40"),
                span("conversation", "Bump", "11:00", "11:20", "c1", "harbour"),
                span("elsewhere", "Settings", "11:20", "11:25"),
            ),
        )

        assertEquals(listOf("Bump" to 50L, "History, Settings" to 15L), rows.map { it.title to it.minutes })
    }

    @Test
    fun theTableShowsAtMostSixRowsLongestFirst() {
        val spans = (1..8).map { span("conversation", "Task $it", "0$it:00", "0$it:${it * 5 + 10}", "c$it") }

        assertEquals(6, activityRows(spans).size)
        assertEquals("Task 8", activityRows(spans).first().title)
    }
}
