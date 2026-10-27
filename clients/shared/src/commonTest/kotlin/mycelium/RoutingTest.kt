package mycelium

import mycelium.api.AccountPrice
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.ModelChoice
import mycelium.api.ModelProvider
import mycelium.api.UsageWindow
import mycelium.state.AmountProblem
import mycelium.state.bestLogin
import mycelium.state.formatAmount
import mycelium.state.formatMoney
import mycelium.state.loginStanding
import mycelium.state.modelChoiceFrom
import mycelium.state.parseAmount
import mycelium.state.priceLabel
import mycelium.state.routedLabel
import mycelium.state.routedStatus
import mycelium.state.routingChoices
import mycelium.state.scheduleAccountChoice
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.time.Instant

private val Now = Instant.parse("2026-10-27T12:00:00Z")

private fun window(name: String, used: Double, resetsAt: String? = null) = UsageWindow(name, used, resetsAt)

private fun login(account: String, vararg windows: UsageWindow, state: LoginState = LoginState.SignedIn, agent: String = "claude") = LoginInfo(agent, account, "token", state, windows = windows.toList())

private fun routed(agent: String, account: String?, routing: String) =
    Conversation("c1", "u", "n1", agent, "/work/app", "Title", ConversationStatus.Done, createdAt = "2026-10-27T10:00:00Z", updatedAt = "2026-10-27T10:00:00Z", account = account, routing = routing)

class RoutingTest {
    @Test
    fun theAccountWithTheMostFiveHourUsageLeftWinsAndTheWeeklyWindowBreaksATie() {
        val logins = listOf(
            login("a", window("five_hour", 60.0), window("seven_day", 10.0)),
            login("b", window("five_hour", 20.0), window("seven_day", 90.0)),
            login("c", window("five_hour", 20.0), window("seven_day", 40.0)),
        )

        assertEquals("c", bestLogin(logins, Now)?.account)
    }

    @Test
    fun anAccountAtALimitIsNotChosenWhileAnotherHasUsage() {
        val logins = listOf(login("full", window("five_hour", 100.0)), login("ok", window("five_hour", 95.0)))

        assertEquals("ok", bestLogin(logins, Now)?.account)
        assertNull(bestLogin(logins.take(1), Now))
    }

    @Test
    fun aWindowThatHasResetCountsAsUnused() {
        val standing = loginStanding(login("a", window("five_hour", 100.0, "2026-10-27T11:00:00Z")), Now)

        assertEquals(false, standing.limited)
        assertEquals(100.0, standing.fiveLeft)
    }

    @Test
    fun signedOutAccountsAreNeverChosen() {
        assertNull(bestLogin(listOf(login("a", window("five_hour", 1.0), state = LoginState.SignedOut)), Now))
    }

    @Test
    fun theAnyModelChoiceHintNamesTheFirstEntryThatCanRun() {
        val logins = listOf(login("work@example.org", window("five_hour", 10.0)), login("codex@example.com", agent = "codex"))
        val list = listOf(ModelChoice(agent = "codex", account = "missing@example.com"), ModelChoice(agent = "claude"))

        val choices = routingChoices(logins, list, "starts on")

        assertEquals("starts on Claude Code", choices.first { it.id == "routing:any_model" }.description)
    }

    @Test
    fun aRoutedConversationShowsTheAccountItIsOnNow() {
        assertEquals("Any account: work@example.org", routedLabel(routed("claude", "work@example.org", "claude_any"), emptyList()))
        assertEquals("Any model: Claude Code, work@example.org", routedLabel(routed("claude", "work@example.org", "any_model"), emptyList()))
        assertEquals(
            "Any model: Ollama · llama4",
            routedLabel(routed("chat", null, "any_model").copy(providerId = "p1", model = "llama4"), listOf(ModelProvider("p1", "openai_compatible", "Ollama", createdAt = "2026-10-01T10:00:00Z"))),
        )
    }

    @Test
    fun theRoutedStatusNamesTheTightestWindowLeft() {
        val status = routedStatus(routed("claude", "work@example.org", "claude_any"), login("work@example.org", window("seven_day", 36.0)))

        assertEquals("Claude Code, any account. On work@example.org now: 64% of the weekly window left", status)
    }

    @Test
    fun aScheduleDefaultsToAnyAccountOnlyWhereNamedAccountsAreSignedIn() {
        val withAccounts = nodeWith(login("a"))

        assertEquals("routing:claude_any", scheduleAccountChoice(withAccounts, "claude", null))
        assertEquals("routing:any_model", scheduleAccountChoice(withAccounts, "claude", "routing:any_model"))
        assertNull(scheduleAccountChoice(nodeWith(), "claude", null))
    }

    @Test
    fun anAddedModelBecomesTheMatchingChoice() {
        assertEquals(ModelChoice(agent = "codex"), modelChoiceFrom("any:codex"))
        assertEquals(ModelChoice(agent = "claude", account = "a@b.c"), modelChoiceFrom("account:claude:a@b.c"))
        assertEquals(ModelChoice(providerId = "p1", model = "opus:5"), modelChoiceFrom("chat:p1:opus:5"))
        assertEquals(ModelChoice(anyModel = true), modelChoiceFrom("any_model"))
    }

    @Test
    fun aPriceReadsWithItsSymbolAndPeriod() {
        assertEquals("£1,800 a year", priceLabel(AccountPrice("claude", "a", 1800.0, "GBP", "yearly")))
        assertEquals("£150.50 a month", priceLabel(AccountPrice("claude", "a", 150.5, "GBP", "monthly")))
        assertEquals("CHF 20", formatMoney(20.0, "CHF"))
        assertEquals("1,234,567.89", formatAmount(1234567.89))
    }

    @Test
    fun anAmountIsANumberWithOptionalGroupingCommas() {
        assertEquals(1800.0, parseAmount("1800"))
        assertEquals(1800.0, parseAmount("1,800"))
        assertEquals(150.5, parseAmount(" 150.50 "))

        for (bad in listOf("", "0", "abc", "1.800,00", "12,34", "-5")) {
            assertNull(parseAmount(bad), bad)
        }

        assertEquals("Enter the amount as a number, like 1800 or 150.50", AmountProblem)
    }

    private fun nodeWith(vararg logins: LoginInfo) = mycelium.api.NodeSummary("n1", "willow", hostname = "willow", os = "linux", arch = "x86_64", version = "v0.11.0", online = true, draining = false, logins = logins.toList())
}
