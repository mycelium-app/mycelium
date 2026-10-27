package mycelium

import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import mycelium.api.ResetCredit
import mycelium.api.ResetCredits
import mycelium.state.bankedResetsSummary
import mycelium.api.UsageWindow
import mycelium.state.AccountOnNode
import mycelium.state.AgentAccount
import mycelium.state.accountChoices
import mycelium.state.agentAccounts
import mycelium.state.chatAccountFirst
import mycelium.state.UnnamedAccount
import mycelium.state.runsOutBeforeReset
import mycelium.state.splitSignedOut
import mycelium.state.usageClockTime
import mycelium.state.usageProjection
import mycelium.state.compactWindowUsage
import mycelium.state.CompactWindowUsage
import mycelium.state.windowName
import mycelium.state.evenPacePercent
import mycelium.state.nextUsageTick
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds
import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertEquals
import kotlin.time.Instant

private val Now = Instant.parse("2026-10-23T14:00:00Z")

private fun utc(millis: Long): String = Instant.fromEpochMilliseconds(millis).toString().replace("Z", "").substring(0, 16)

private fun node(id: String, name: String, vararg logins: LoginInfo) = NodeSummary(id, name, hostname = name, os = "linux", arch = "x86_64", version = "0.4.0", online = true, draining = false, logins = logins.toList())

class AccountUsageTest {
    @Test
    fun oneAccountSignedInOnSeveralNodesIsOneEntryListingEachNodeWithTheFreshestUsage() {
        val older = LoginInfo("claude", "sam@example.com", "token", LoginState.SignedIn, windows = listOf(UsageWindow("five_hour", 10.0)), usageReadAt = "2026-10-23T10:00:00Z")
        val newer = LoginInfo("claude", "sam@example.com", "token", LoginState.Expired, windows = listOf(UsageWindow("five_hour", 40.0)), usageReadAt = "2026-10-23T12:00:00Z")

        val accounts = agentAccounts(listOf(node("n2", "nas", newer), node("n1", "laptop", older)))

        assertEquals(1, accounts.size)
        assertEquals(listOf(AccountOnNode("laptop", LoginState.SignedIn), AccountOnNode("nas", LoginState.Expired)), accounts.single().nodes)
        assertEquals(40.0, accounts.single().windows.single().usedPercent)
        assertEquals("2026-10-23T12:00:00Z", accounts.single().usageReadAt)
    }

    @Test
    fun theBankedResetsComeFromTheFreshestLogin() {
        val stale = LoginInfo("codex", "sam@example.com", "token", LoginState.SignedIn, windows = listOf(UsageWindow("five_hour", 10.0)), usageReadAt = "2026-10-23T10:00:00Z", resetCredits = ResetCredits(1))
        val fresh = LoginInfo("codex", "sam@example.com", "token", LoginState.SignedIn, windows = listOf(UsageWindow("five_hour", 20.0)), usageReadAt = "2026-10-23T12:00:00Z", resetCredits = ResetCredits(3))

        val accounts = agentAccounts(listOf(node("n1", "laptop", stale), node("n2", "nas", fresh)))

        assertEquals(3, accounts.single().resetCredits?.availableCount)
    }

    @Test
    fun bankedResetsAreSummarisedWithTheNextExpiryWhenOneIsKnown() {
        val utc: (Long) -> String? = { millis -> utc(millis) }
        val credits = ResetCredits(2, listOf(ResetCredit("b", "2026-12-01T00:00:00Z"), ResetCredit("a", "2026-11-14T09:00:00Z"), ResetCredit("c")))

        assertEquals("2 resets banked, next expires 14 Nov", bankedResetsSummary(credits, utc))
        assertEquals("1 reset banked", bankedResetsSummary(ResetCredits(1), utc))
        assertEquals("1 reset banked", bankedResetsSummary(ResetCredits(1, listOf(ResetCredit("a"))), utc))
    }

    @Test
    fun accountNamesThatDifferOnlyInCaseOrSpacingAreOneAccount() {
        val first = LoginInfo("claude", "Sam@Example.com", "token", LoginState.SignedIn)
        val second = LoginInfo("claude", " sam@example.com ", "token", LoginState.SignedIn)

        val accounts = agentAccounts(listOf(node("n1", "laptop", first), node("n2", "nas", second)))

        assertEquals(listOf("Sam@Example.com"), accounts.map { it.label })
        assertEquals(listOf("laptop", "nas"), accounts.single().nodes.map { it.nodeName })
    }

    @Test
    fun loginsWithoutANameStayPerNodeAndDifferentAgentsStaySeparate() {
        val claude = LoginInfo("claude", null, "token", LoginState.SignedIn)
        val codex = LoginInfo("codex", "sam@example.com", "token", LoginState.SignedIn)

        val accounts = agentAccounts(listOf(node("n1", "laptop", claude, codex), node("n2", "nas", claude)))

        assertEquals(listOf("claude" to "laptop", "claude" to "nas", "codex" to "laptop"), accounts.map { it.agent to it.nodes.single().nodeName })
    }

    @Test
    fun signedOutLoginsWithoutANameBecomeSortedNodesAndEveryOtherAccountStays() {
        val named = AgentAccount("codex", "sam@example.com", "oauth_token", listOf(AccountOnNode("willow", LoginState.SignedIn)))
        val expired = AgentAccount("codex", UnnamedAccount, "oauth_token", listOf(AccountOnNode("nas", LoginState.Expired)))
        val outOnOrion = AgentAccount("codex", UnnamedAccount, "oauth_token", listOf(AccountOnNode("orion", LoginState.SignedOut)))
        val outOnAtlas = AgentAccount("codex", UnnamedAccount, "oauth_token", listOf(AccountOnNode("atlas", LoginState.SignedOut)))

        val split = splitSignedOut(listOf(outOnOrion, named, expired, outOnAtlas))

        assertEquals(listOf(named, expired), split.accounts)
        assertEquals(listOf("atlas", "orion"), split.signedOutNodes)
    }

    @Test
    fun unusedGeneratedSignedOutLoginsDoNotBecomeAccounts() {
        val unused = LoginInfo("codex", "account-1234", "oauth_token", LoginState.SignedOut)
        val used = unused.copy(account = "account-5678", windows = listOf(UsageWindow("five_hour", 20.0)))
        val named = unused.copy(account = "sam@example.com")
        val signedIn = unused.copy(account = "account-9012", state = LoginState.SignedIn)

        val accounts = agentAccounts(listOf(node("n1", "laptop", unused, used, named, signedIn)))

        assertEquals(listOf("account-5678", "account-9012", "sam@example.com"), accounts.map { it.label })
    }

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

        val projection = usageProjection(window, ::utc, Now).orEmpty()

        assertContains(projection, "18:40")
        assertContains(projection, "22:30")
    }

    @Test
    fun withoutAKnownLocalTimeTheProjectionSaysHowLongAway() {
        val window = UsageWindow("five_hour", 64.0, resetsAt = "2026-10-23T22:30:00Z", ratePerHour = 12.0, runsOutAt = "2026-10-23T18:40:00Z")

        val projection = usageProjection(window, { null }, Now).orEmpty()

        assertContains(projection, "4h")
        assertContains(projection, "8h")
    }

    @Test
    fun aWindowThatLastsUntilItResetsSaysSo() {
        val window = UsageWindow("seven_day", 20.0, resetsAt = "2026-10-28T09:00:00Z", ratePerHour = 0.2)

        assertContains(usageProjection(window, ::utc, Now).orEmpty(), "28 Oct 09:00")
    }

    @Test
    fun aWindowWithNoRateOnlyGivesItsResetTime() {
        assertEquals("Resets 22:30", usageProjection(UsageWindow("five_hour", 5.0, resetsAt = "2026-10-23T22:30:00Z"), ::utc, Now))
        assertEquals("", usageProjection(UsageWindow("five_hour", 5.0), ::utc, Now))
    }

    @Test
    fun aCompactWindowGivesItsForecastAndTimeLeft() {
        val window = UsageWindow("five_hour", 64.0, resetsAt = "2026-10-23T19:00:00Z", ratePerHour = 12.0, runsOutAt = "2026-10-23T17:00:00Z")

        assertEquals(CompactWindowUsage(64, "runs out in 3h", "64% used, 5h left", true), compactWindowUsage(window, ::utc, Now))
    }

    @Test
    fun aCompactWindowThatLastsUntilItResetsHasNoForecast() {
        val window = UsageWindow("seven_day", 20.0, resetsAt = "2026-10-28T09:00:00Z", ratePerHour = 0.2)

        assertEquals(CompactWindowUsage(20, null, "20% used, 4d left", false), compactWindowUsage(window, ::utc, Now))
    }

    @Test
    fun aFullWindowDoesNotSayWhenItRunsOut() {
        val window = UsageWindow("five_hour", 100.0, resetsAt = "2026-10-23T14:06:00Z", ratePerHour = 20.0, runsOutAt = "2026-10-23T13:57:00Z")

        assertEquals(CompactWindowUsage(100, null, "100% used, 6m left", false), compactWindowUsage(window, ::utc, Now))
    }

    @Test
    fun aWindowWhoseResetHasPassedShowsAsReset() {
        val window = UsageWindow("five_hour", 100.0, resetsAt = "2026-10-23T13:03:00Z", ratePerHour = 20.0, runsOutAt = "2026-10-23T12:57:00Z")

        assertEquals(CompactWindowUsage(0, null, "0% used, reset 13:03", false), compactWindowUsage(window, ::utc, Now))
    }

    @Test
    fun evenPaceIsTheShareOfTheWindowThatHasElapsed() {
        assertEquals(50, evenPacePercent(UsageWindow("five_hour", 5.0, resetsAt = "2026-10-23T16:30:00Z"), Now))
        assertEquals(50, evenPacePercent(UsageWindow("seven_day", 5.0, resetsAt = "2026-10-27T02:00:00Z"), Now))
        assertEquals(null, evenPacePercent(UsageWindow("five_hour", 5.0), Now))
        assertEquals(null, evenPacePercent(UsageWindow("five_hour", 5.0, resetsAt = "2026-10-23T13:00:00Z"), Now))
        assertEquals(null, evenPacePercent(UsageWindow("other", 5.0, resetsAt = "2026-10-23T16:00:00Z"), Now))
        assertEquals(null, evenPacePercent(UsageWindow("five_hour", 5.0, resetsAt = "bad date"), Now))
        assertEquals(null, evenPacePercent(UsageWindow("five_hour", 5.0, resetsAt = "2026-10-24T14:00:00Z"), Now))
    }

    @Test
    fun theCardWakesAtTheNextResetOrWithinAMinute() {
        val soon = UsageWindow("five_hour", 5.0, resetsAt = "2026-10-23T14:00:20Z")
        val later = UsageWindow("seven_day", 5.0, resetsAt = "2026-10-25T14:00:00Z")
        val passed = UsageWindow("five_hour", 5.0, resetsAt = "2026-10-23T13:00:00Z")

        assertEquals(20.seconds, nextUsageTick(listOf(later, soon), Now))
        assertEquals(1.minutes, nextUsageTick(listOf(later, passed), Now))
        assertEquals(1.minutes, nextUsageTick(emptyList(), Now))
        assertEquals(1.seconds, nextUsageTick(listOf(soon.copy(resetsAt = "2026-10-23T14:00:00.500Z")), Now))
        assertEquals(1.minutes, nextUsageTick(listOf(soon.copy(resetsAt = "bad date")), Now))
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

    private val Ann = AgentAccount("claude", "ann@example.com", "token", listOf(AccountOnNode("laptop", LoginState.SignedIn)))
    private val Work = AgentAccount("claude", "work@example.com", "token", listOf(AccountOnNode("laptop", LoginState.SignedIn), AccountOnNode("atlas", LoginState.SignedIn)))
    private val Codex = AgentAccount("codex", "ann@example.com", "token", listOf(AccountOnNode("laptop", LoginState.SignedIn)))

    @Test
    fun theAccountTheOpenChatUsesComesFirstAndIsMarked() {
        val ordered = chatAccountFirst(listOf(Ann, Work, Codex), "claude", "atlas", "work@example.com")

        assertEquals(listOf(Work to true, Ann to false, Codex to false), ordered)
    }

    @Test
    fun aChatWithoutAnAccountMatchesTheFirstAccountForItsAgentOnItsNode() {
        val ordered = chatAccountFirst(listOf(Ann, Work, Codex), "codex", "laptop", null)

        assertEquals(listOf(Codex to true, Ann to false, Work to false), ordered)
    }

    @Test
    fun withoutAMatchingAccountTheOrderIsUnchangedAndNothingIsMarked() {
        assertEquals(listOf(Ann to false, Work to false), chatAccountFirst(listOf(Ann, Work), null, null, null))
        assertEquals(listOf(Ann to false, Work to false), chatAccountFirst(listOf(Ann, Work), "claude", "elsewhere", null))
    }

    @Test
    fun aWindowRunsOutBeforeResetOnlyWhenTheProjectedTimeIsEarlier() {
        assertEquals(true, runsOutBeforeReset(UsageWindow("five_hour", 80.0, resetsAt = "2026-10-23T22:30:00Z", runsOutAt = "2026-10-23T18:40:00Z")))
        assertEquals(false, runsOutBeforeReset(UsageWindow("five_hour", 80.0, resetsAt = "2026-10-23T22:30:00Z")))
        assertEquals(false, runsOutBeforeReset(UsageWindow("five_hour", 80.0, runsOutAt = "2026-10-23T18:40:00Z")))
    }
}
