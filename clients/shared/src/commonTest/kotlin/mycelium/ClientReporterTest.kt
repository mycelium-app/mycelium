package mycelium

import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.ClientEventReport
import mycelium.state.ClientReporter
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.time.Instant

private val At = Instant.parse("2026-11-15T09:00:00Z")

class ClientReporterTest {
    @Test
    fun eventsReportedTogetherGoOutInOneBatchWithTheSessionClientAndVersion() = runTest {
        val sent = mutableListOf<ClientEventReport>()
        val reporter = ClientReporter("web", "tab-1", backgroundScope, { sent += it }, now = { At })

        reporter.report("error", "Size(1011 x 2147483647) is out of range")
        reporter.report("connection", "closed", mapOf("close_code" to 1006, "reason" to null))
        advanceTimeBy(10_001)
        runCurrent()

        val report = sent.single()
        assertEquals(Triple("tab-1", "web", ClientVersion), Triple(report.sessionId, report.client, report.version))
        assertEquals(listOf("Size(1011 x 2147483647) is out of range", "closed"), report.events.map { it.message })
        assertEquals(JsonPrimitive(1006), report.events[1].detail["close_code"])
        assertFalse("reason" in report.events[1].detail)
        assertEquals("2026-11-15T09:00:00Z", report.events[0].at)
    }

    @Test
    fun aReportThatCannotBeSentIsDroppedAndLaterEventsStillGoOut() = runTest {
        val sent = mutableListOf<ClientEventReport>()
        var failing = true
        val reporter = ClientReporter("desktop", "process-1", backgroundScope, { report ->
            if (failing) {
                failing = false
                error("offline")
            }

            sent += report
        }, now = { At })

        reporter.report("connection", "failed")
        advanceTimeBy(10_001)
        runCurrent()
        reporter.report("connection", "open")
        advanceTimeBy(13_002)
        runCurrent()

        assertEquals(listOf("open"), sent.flatMap { report -> report.events.map { it.message } })
    }

    @Test
    fun onlyTheNewestTwoHundredEventsAreKeptWhileTheyWait() = runTest {
        val sent = mutableListOf<ClientEventReport>()
        val reporter = ClientReporter("android", "process-1", backgroundScope, { sent += it }, now = { At })

        repeat(250) { index -> reporter.report("visibility", "event $index") }
        advanceTimeBy(60_000)
        runCurrent()

        val messages = sent.flatMap { report -> report.events.map { it.message } }
        assertEquals(200, messages.size)
        assertEquals("event 50", messages.first())
        assertEquals("event 249", messages.last())
    }
}
