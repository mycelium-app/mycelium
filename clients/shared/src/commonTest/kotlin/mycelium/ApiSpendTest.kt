package mycelium

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.time.Instant
import mycelium.api.ApiCost
import mycelium.api.ModelProviderUsage
import mycelium.api.OpenRouterSpend
import mycelium.state.SpendFigure
import mycelium.state.apiSpendRow
import mycelium.state.tokenCount
import mycelium.state.usdAmount

private val November = Instant.parse("2026-11-11T10:00:00Z")

class ApiSpendTest {
    @Test
    fun amountsAreInDollarsAndCents() {
        assertEquals("$6.40", usdAmount(6.4))
        assertEquals("-$0.10", usdAmount(-0.1))
    }

    @Test
    fun tokenCountsAreShortened() {
        assertEquals("980", tokenCount(980))
        assertEquals("12.3k", tokenCount(12_300))
        assertEquals("1.2M", tokenCount(1_200_000))
        assertEquals("2.0M", tokenCount(2_000_000))
    }

    @Test
    fun anAnthropicProviderShowsTodayAndTheMonthSoFar() {
        val provider = ModelProviderUsage("p1", "Anthropic", "2026-11-11T09:58:00Z", apiCost = ApiCost(6.4, 142.18, 1_200_000))
        val row = apiSpendRow(provider, November)

        assertEquals("Anthropic", row.name)
        assertEquals(SpendFigure("$6.40", "today"), row.today)
        assertEquals(SpendFigure("$142.18", "in Nov"), row.month)
        assertEquals("Today $6.40 and 1.2M tokens. So far this month $142.18.", row.tooltip)
    }

    @Test
    fun aRefusedKeyShowsTheRefusedStateInsteadOfFigures() {
        val provider = ModelProviderUsage("p1", "Anthropic", status = "refused", apiCost = ApiCost(6.4, 142.18))

        assertEquals("Admin key refused", apiSpendRow(provider, November).problem)
    }

    @Test
    fun aProviderThatCouldNotBeReadKeepsItsLastGoodFigures() {
        val provider = ModelProviderUsage("p1", "Anthropic", status = "unreadable", apiCost = ApiCost(6.4, 142.18))
        val row = apiSpendRow(provider, November)

        assertEquals(null, row.problem)
        assertEquals(SpendFigure("$142.18", "in Nov"), row.month)
    }

    @Test
    fun aProviderWithNothingReadSaysSoAndOneThatFailedSaysItCouldNotBeRead() {
        assertEquals("Not reported yet", apiSpendRow(ModelProviderUsage("p1", "Anthropic"), November).problem)
        assertEquals("Could not be read", apiSpendRow(ModelProviderUsage("p1", "Anthropic", status = "unreadable"), November).problem)
    }

    @Test
    fun openRouterUsesTheSameRowWithItsDailyAndMonthlySpend() {
        val provider = ModelProviderUsage("p1", "Work key", spend = OpenRouterSpend(total = 12.345, daily = 0.125, monthly = 8.5))
        val row = apiSpendRow(provider, November)

        assertEquals("OpenRouter: Work key", row.name)
        assertEquals(SpendFigure("$0.13", "today"), row.today)
        assertEquals(SpendFigure("$8.50", "in Nov"), row.month)
    }

    @Test
    fun openRouterWithoutPeriodsShowsItsTotal() {
        val row = apiSpendRow(ModelProviderUsage("p1", "OpenRouter", spend = OpenRouterSpend(total = 2.39)), November)

        assertEquals("OpenRouter", row.name)
        assertEquals(SpendFigure("$2.39", "in total"), row.today)
    }
}
