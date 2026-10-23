package mycelium

import mycelium.api.Activity
import mycelium.api.ActivityRow
import mycelium.api.MyceliumJson
import mycelium.state.dayLabel
import mycelium.state.formatCost
import mycelium.state.formatDuration
import mycelium.state.formatTokens
import mycelium.state.tokensPerDaySummary
import mycelium.state.usageNotice
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

private fun totals(turns: Long, reported: Long, tokens: Long = 0) = ActivityRow("", "", turns = turns, reportedTurns = reported, inputTokens = tokens)

private fun activity(totals: ActivityRow, unreported: List<String> = emptyList(), days: List<ActivityRow> = emptyList()) = Activity("7d", totals, days = days, unreportedAgents = unreported)

class ActivityTest {
    @Test
    fun tokensAreAbbreviatedWithOneDecimalWhereItMatters() {
        assertEquals("999", formatTokens(999))
        assertEquals("1.2k", formatTokens(1_234))
        assertEquals("12k", formatTokens(12_000))
        assertEquals("1.5M", formatTokens(1_500_000))
        assertEquals("182M", formatTokens(182_000_000))
        assertEquals("2.5B", formatTokens(2_500_000_000))
    }

    @Test
    fun costIsApproximateRoundedToCentsAndThenToDollars() {
        assertEquals("~$0.15", formatCost(0.15))
        assertEquals("~$4.05", formatCost(4.05))
        assertEquals("~$1,940", formatCost(1940.2))
        assertEquals("Not reported", formatCost(null))
    }

    @Test
    fun timeWorkingIsWrittenInTheLargestTwoUnits() {
        assertEquals("45s", formatDuration(45_000))
        assertEquals("2m 10s", formatDuration(130_000))
        assertEquals("3h 12m", formatDuration(11_520_000))
    }

    @Test
    fun aDayIsWrittenAsDayAndMonth() {
        assertEquals("2 Oct", dayLabel("2026-10-02"))
        assertEquals("nonsense", dayLabel("nonsense"))
    }

    @Test
    fun theUsageNoticeSaysWhichAgentsReportNothingAndStaysQuietWhenAllDo() {
        assertNull(usageNotice(activity(totals(0, 0))))
        assertNull(usageNotice(activity(totals(4, 4))))
        assertEquals("Codex did not report token usage in this range, so only its turns and time working are counted.", usageNotice(activity(totals(4, 2), listOf("codex"))))
        assertEquals("Claude Code and Codex did not report token usage in this range, so only their turns and time working are counted.", usageNotice(activity(totals(4, 2), listOf("claude", "codex"))))
        assertEquals("No agent reported token usage in this range, so only turns and time working are counted.", usageNotice(activity(totals(4, 0), listOf("claude"))))
    }

    @Test
    fun theChartSummaryNamesThePeakDay() {
        val days = listOf(ActivityRow("2026-10-01", "2026-10-01", inputTokens = 100), ActivityRow("2026-10-02", "2026-10-02", inputTokens = 900))

        assertEquals("Peak 900 on 2 Oct, 1k in total.", tokensPerDaySummary(activity(totals(2, 2, 1000), days = days)))
        assertEquals("No tokens were reported on these days.", tokensPerDaySummary(activity(totals(0, 0), days = days.map { it.copy(inputTokens = 0) })))
    }

    @Test
    fun anActivityAnswerIsDecodedWithItsRowsAndNullCost() {
        val json = """{"range":"7d","totals":{"key":"","name":"","turns":3,"reported_turns":0,"working_ms":9000,"input_tokens":0,"output_tokens":0,"cache_read_tokens":0,"cache_write_tokens":0},
            "days":[{"key":"2026-10-02","name":"2026-10-02","turns":3}],"unreported_agents":["codex"]}"""

        val decoded = MyceliumJson.decodeFromString(Activity.serializer(), json)

        assertEquals(3, decoded.totals.turns)
        assertNull(decoded.totals.costUsd)
        assertEquals(listOf("codex"), decoded.unreportedAgents)
    }
}
