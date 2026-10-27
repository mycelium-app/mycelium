package mycelium

import kotlinx.serialization.json.Json
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.state.ThreadItem
import mycelium.state.buildThread
import mycelium.state.clockTime
import mycelium.state.limitName
import mycelium.state.resetBefore
import mycelium.state.resumeTimeAfter
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.time.Instant

private fun error(payload: String) = Event("c1", 1, EventKinds.Error, Json.parseToJsonElement(payload))

class UsageLimitTest {
    private val now = Instant.parse("2026-10-23T12:00:00Z")

    @Test
    fun anErrorEventWithAUsageLimitCarriesItsWindowAndReset() {
        val failure = assertIs<ThreadItem.Failure>(buildThread(listOf(error("""{"message":"x","usage_limit":{"window":"seven_day","resets_at":"2026-10-28T09:00:00Z"}}"""))).single())

        assertEquals("seven_day", failure.limit?.window)
        assertEquals("2026-10-28T09:00:00Z", failure.limit?.resetsAt)
    }

    @Test
    fun aLimitWithoutAResetTimeHasNone() {
        val failure = assertIs<ThreadItem.Failure>(buildThread(listOf(error("""{"message":"x","usage_limit":{"window":"usage","resets_at":null}}"""))).single())

        assertNull(failure.limit?.resetsAt)
    }

    @Test
    fun anOrdinaryErrorHasNoLimit() {
        assertNull(assertIs<ThreadItem.Failure>(buildThread(listOf(error("""{"message":"The keeper is unreachable."}"""))).single()).limit)
    }

    @Test
    fun windowsAreNamedForThePerson() {
        assertEquals(listOf("5-hour limit", "weekly limit", "weekly Opus limit", "usage limit"), listOf("five_hour", "seven_day", "seven_day_opus", "usage").map(::limitName))
    }

    @Test
    fun theResetIsAMinuteBeforeTheResume() {
        assertEquals("2026-10-23T15:30:00Z", resetBefore("2026-10-23T15:31:00Z"))
    }

    @Test
    fun aSecondLimitReplyForTheSameResetReplacesTheFirstCard() {
        val limit = """{"message":"x","usage_limit":{"window":"five_hour","resets_at":"2026-10-23T15:30:00Z"}}"""
        val thread = buildThread(listOf(error(limit), Event("c1", 2, EventKinds.UserMessage, Json.parseToJsonElement("""{"text":"cont."}""")), Event("c1", 3, EventKinds.Error, Json.parseToJsonElement(limit))))

        assertEquals(listOf("e2", "e3"), thread.map { it.key })
    }

    @Test
    fun aWorkflowAgentStoppedByTheLimitIsPausedNotFailedAndResumesWhenItStartsAgain() {
        val started = """{"type":"started","key":"k1","label":"Build","phase":"Implement"}"""
        val failed = """{"type":"failed","key":"k1","reason":"You've hit your session limit"}"""
        val limit = ""","usage_limit":{"window":"five_hour","resets_at":"2026-10-23T15:30:00Z"}"""

        fun line(seq: Long, body: String, extra: String = "") = Event("c1", seq, EventKinds.Workflow, Json.parseToJsonElement("""{"run_id":"r1","line":$body$extra}"""))

        val paused = assertIs<ThreadItem.Workflow>(buildThread(listOf(line(1, started), line(2, failed, limit))).single()).progress
        val resumed = assertIs<ThreadItem.Workflow>(buildThread(listOf(line(1, started), line(2, failed, limit), line(3, started))).single()).progress

        assertEquals(listOf(true, false, "2026-10-23T15:30:00Z"), listOf(paused.paused, paused.agents.single().failed, paused.pausedUntil))
        assertEquals(listOf(false, false), listOf(resumed.paused, resumed.agents.single().paused))
    }

    @Test
    fun resumingWaitsAMinutePastTheReset() {
        assertEquals("2026-10-23T15:31:00Z", resumeTimeAfter("2026-10-23T15:30:00Z"))
    }

    @Test
    fun aTimeTodayIsShownAsTheClockAndALaterDayWithItsDate() {
        val utc: (Long) -> String? = { millis -> Instant.fromEpochMilliseconds(millis).toString().substringBefore('Z') }

        assertEquals("16:30", clockTime("2026-10-23T16:30:00Z", now, utc))
        assertEquals("28 Oct 09:00", clockTime("2026-10-28T09:00:00Z", now, utc))
    }

    @Test
    fun theClockFollowsTheLocalZoneTheDeviceReports() {
        val plusOneHour: (Long) -> String? = { millis -> Instant.fromEpochMilliseconds(millis + 3_600_000).toString().substringBefore('Z') }

        assertEquals("17:30", clockTime("2026-10-23T16:30:00Z", now, plusOneHour))
    }
}
