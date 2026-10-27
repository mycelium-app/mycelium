package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.test.junit4.ComposeContentTestRule
import mycelium.ui.Meter
import mycelium.ui.MyTheme
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.UsageWindow
import mycelium.state.AgentAccount
import mycelium.api.ModelProviderUsage
import mycelium.api.OpenRouterSpend
import mycelium.ui.UsageCard
import mycelium.api.MyceliumApi
import mycelium.api.ResetCredit
import mycelium.api.ResetCredits
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.time.Duration.Companion.minutes
import kotlin.test.assertTrue
import kotlin.test.assertEquals
import java.util.concurrent.atomic.AtomicInteger

private object UsageCardPlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

private const val UsageCardNodes = """[{"id":"n1","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"v0.1.13","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],
"logins":[
 {"agent":"claude","account":"ann@example.net","method":"oauth_token","state":"signed_in","usage_read_at":"2099-01-01T10:00:00Z",
  "windows":[{"name":"seven_day","used_percent":10.0,"resets_at":"2099-01-05T10:00:00Z"}]},
 {"agent":"claude","account":"quiet@example.org","method":"oauth_token","state":"signed_in"},
 {"agent":"claude","account":"work@example.com","method":"oauth_token","state":"signed_in","usage_read_at":"2099-01-01T10:00:00Z",
  "windows":[{"name":"five_hour","used_percent":40.0,"resets_at":"2099-01-01T22:30:00Z"}]}],
"sessions":[]}]"""

private const val UsageCardConversations = """[{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Open chat","status":"done","account":"work@example.com",
"acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}]"""

class UsageCardTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun showShell(openRouter: Boolean = false, failAfterFirst: Boolean = false): AtomicInteger {
        val usageRequests = AtomicInteger()
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            if (path.endsWith("/model-provider-usage") && usageRequests.incrementAndGet() > 1 && failAfterFirst) {
                return@MockEngine respond("{}", HttpStatusCode.BadGateway, json)
            }

            val body = when {
                path.endsWith("/model-provider-usage") -> if (openRouter) """[{"provider_id":"p1","name":"Work key","read_at":"2099-01-01T10:00:00Z","spend":{"total":12.345,"daily":0.125,"weekly":2.0,"monthly":8.5,"limit_remaining":87.655}}]""" else "[]"
                path.endsWith("/nodes") -> if (openRouter) "[]" else UsageCardNodes
                path.endsWith("/conversations") -> UsageCardConversations
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, UsageCardPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }

        return usageRequests
    }

    private fun codexWithBankedResets() = AgentAccount("codex", "work@example.com", "device_auth", emptyList(), windows = listOf(UsageWindow("five_hour", 5.0)), resetCredits = ResetCredits(2))

    @Test
    fun anOverspentKeyShowsItsNegativeRemainingLimit() {
        rule.setContent {
            MyceliumTheme(true) {
                UsageCard(emptyList(), providers = listOf(ModelProviderUsage("p1", "Overspent key", spend = OpenRouterSpend(total = 1.1, limitRemaining = -0.1))))
            }
        }

        rule.onNodeWithText("-$0.10").performScrollTo().assertIsDisplayed()
    }

    @Test
    fun aKeyWithADailyLimitShowsTodaysSpendAgainstItAndTheTotalBelow() {
        rule.setContent {
            MyceliumTheme(true) {
                UsageCard(emptyList(), providers = listOf(ModelProviderUsage("p1", "OpenRouter", spend = OpenRouterSpend(total = 2.39, daily = 2.34, limit = 5.0, limitReset = "daily"))))
            }
        }

        rule.onNodeWithText("OpenRouter").assertIsDisplayed()
        rule.onNodeWithText("OpenRouter: OpenRouter").assertDoesNotExist()
        rule.onNodeWithText("Daily limit").assertIsDisplayed()
        rule.onNodeWithText("$2.34 / $5.00").assertIsDisplayed()
        rule.onNodeWithContentDescription("Daily limit 47% used").assertExists()
        rule.onNodeWithText("$2.39").assertIsDisplayed()
    }

    @Test
    fun lastUpdatedLinesUpWithTheWindowValuesAtTheRightAndALongForecastLeavesTheValueWhole() {
        val window = UsageWindow("seven_day", 17.0, resetsAt = "2099-01-07T10:00:00Z", ratePerHour = 1.0, runsOutAt = "2099-01-03T01:37:00Z")
        val account = AgentAccount("codex", "work@example.com", "oauth_token", emptyList(), windows = listOf(window), usageReadAt = "2026-10-25T10:00:00Z")

        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(320.dp).height(600.dp)) {
                    UsageCard(listOf(account to false))
                }
            }
        }

        val updated = rule.onNodeWithText("last updated", substring = true).getUnclippedBoundsInRoot()
        val value = rule.onNodeWithText("17% used", substring = true).fetchSemanticsNode()

        assertEquals(updated.right, value.boundsInRoot.right.toDp(rule), 1.dp)
        assertTrue(value.config[SemanticsProperties.Text].single().text.endsWith("left"))
    }

    @Test
    fun aCodexAccountWithBankedResetsShowsTheCountAndNextExpiry() {
        val credits = ResetCredits(2, listOf(ResetCredit("a", "2099-11-14T09:00:00Z")))
        val account = AgentAccount("codex", "work@example.com", "device_auth", emptyList(), windows = listOf(UsageWindow("five_hour", 5.0)), resetCredits = credits)

        rule.setContent { MyceliumTheme(true) { UsageCard(listOf(account to false)) } }

        rule.onNodeWithText("2 resets banked, next expires 14 Nov").assertIsDisplayed()
    }

    @Test
    fun anAccountWithNoBankedResetsHasNoUseResetButton() {
        val account = AgentAccount("codex", "work@example.com", "device_auth", emptyList(), windows = listOf(UsageWindow("five_hour", 5.0)), resetCredits = ResetCredits(0))

        rule.setContent { MyceliumTheme(true) { UsageCard(listOf(account to false), onUseReset = { _, _ -> "reset" }) } }

        rule.onNodeWithText("Use reset").assertDoesNotExist()
        rule.onNodeWithText("banked", substring = true).assertDoesNotExist()
    }

    @Test
    fun useResetAsksFirstAndCancelSpendsNothing() {
        val calls = mutableListOf<String>()

        rule.setContent { MyceliumTheme(true) { UsageCard(listOf(codexWithBankedResets() to false), onUseReset = { _, key -> calls.add(key); "reset" }) } }
        rule.onNodeWithText("Use reset").performClick()

        rule.onNodeWithText("Use a banked reset?").assertIsDisplayed()
        rule.onNodeWithText("Cancel").performClick()

        rule.onNodeWithText("Use a banked reset?").assertDoesNotExist()
        assertEquals(emptyList(), calls)
    }

    @Test
    fun confirmingUseResetCallsItOnceAndShowsTheOutcome() {
        val calls = mutableListOf<String>()

        rule.setContent { MyceliumTheme(true) { UsageCard(listOf(codexWithBankedResets() to false), onUseReset = { _, key -> calls.add(key); "reset" }) } }
        rule.onNodeWithText("Use reset").performClick()
        rule.onAllNodesWithText("Use reset").onLast().performClick()
        rule.waitUntil { rule.onAllNodesWithText("Reset used.").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(1, calls.size)
        rule.onNodeWithText("Use a banked reset?").assertDoesNotExist()
    }

    @Test
    fun aFailedUseResetShowsWhyAndRetriesWithTheSameKey() {
        val keys = mutableListOf<String>()

        rule.setContent {
            MyceliumTheme(true) {
                UsageCard(listOf(codexWithBankedResets() to false), onUseReset = { _, key -> keys.add(key); if (keys.size == 1) error("Codex did not answer.") else "reset" })
            }
        }
        rule.onNodeWithText("Use reset").performClick()
        rule.onAllNodesWithText("Use reset").onLast().performClick()
        rule.waitUntil { rule.onAllNodesWithText("Codex did not answer.").fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText("Use reset").onLast().performClick()
        rule.waitUntil { rule.onAllNodesWithText("Reset used.").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(2, keys.size)
        assertEquals(keys[0], keys[1])
    }

    @Test
    fun aWindowBarSaysWhereAnEvenPaceWouldBe() {
        val resets = (kotlin.time.Clock.System.now() + 150.minutes).toString()
        val account = AgentAccount("claude", "work@example.com", "oauth_token", emptyList(), windows = listOf(UsageWindow("five_hour", 5.0, resetsAt = resets)))

        rule.setContent { MyceliumTheme(true) { UsageCard(listOf(account to false)) } }

        rule.onNodeWithContentDescription("5-hour window 5% used, even pace 50%").assertExists()
    }

    @Test
    fun aFullWeeklyWindowHidesTheFiveHourWindow() {
        val windows = listOf(UsageWindow("five_hour", 5.0, resetsAt = "2099-01-01T15:00:00Z"), UsageWindow("seven_day", 100.0, resetsAt = "2099-01-03T10:00:00Z"))
        val account = AgentAccount("claude", "work@example.com", "oauth_token", emptyList(), windows = windows)

        rule.setContent { MyceliumTheme(true) { UsageCard(listOf(account to false)) } }

        rule.onNodeWithText("Weekly window").assertIsDisplayed()
        rule.onNodeWithText("5-hour window").assertDoesNotExist()
    }

    @Test
    fun aBarAtNinetyPercentOrMoreIsRed() {
        var failed = Color.Unspecified

        rule.setContent {
            MyceliumTheme(true) {
                failed = MyTheme.colors.statusFailed
                Box(Modifier.width(200.dp)) {
                    Meter(98, "Weekly window 98% used")
                }
            }
        }

        val image = rule.onNodeWithContentDescription("Weekly window 98% used").captureToImage().toPixelMap()

        assertEquals(failed, image[image.width / 2, image.height / 2])
    }

    @Test
    fun theProviderTheChatUsesComesBeforeTheAccounts() {
        val account = AgentAccount("claude", "work@example.com", "oauth_token", emptyList(), windows = listOf(UsageWindow("five_hour", 40.0)))

        rule.setContent {
            MyceliumTheme(true) {
                UsageCard(listOf(account to false), providers = listOf(ModelProviderUsage("p1", "Work key", spend = OpenRouterSpend(total = 1.0))), currentProviderId = "p1")
            }
        }

        val providerTop = rule.onNodeWithText("OpenRouter: Work key").getUnclippedBoundsInRoot().top
        val accountTop = rule.onNodeWithText("example.com").getUnclippedBoundsInRoot().top

        assertTrue(providerTop < accountTop)
    }

    @Test
    fun anUnusedKeyShowsZeroSpendAndUnreportedPeriodsStayUnknown() {
        rule.setContent {
            MyceliumTheme(true) {
                UsageCard(emptyList(), providers = listOf(ModelProviderUsage("p1", "Unused key", spend = OpenRouterSpend(total = 0.0))))
            }
        }

        rule.onNodeWithText("$0.00").assertIsDisplayed()
        rule.onAllNodes(hasText("Not reported")).fetchSemanticsNodes().let { assertEquals(1, it.size) }
        rule.onNodeWithText("Key limit remaining").assertDoesNotExist()
        rule.onAllNodes(hasText("last updated", substring = true)).fetchSemanticsNodes().let { assertEquals(0, it.size) }
    }

    @Test
    fun openRouterKeySpendAppearsAlongsideAccountUsage() {
        val requests = showShell(openRouter = true, failAfterFirst = true)
        assertEquals(0, requests.get())
        rule.onNodeWithContentDescription("Usage").performClick()
        rule.waitUntil { rule.onAllNodes(hasText("OpenRouter: Work key")).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("$12.35").assertIsDisplayed()
        rule.onNodeWithText("$0.13").assertIsDisplayed()
        rule.onNodeWithText("$87.66").assertIsDisplayed()
        rule.onNodeWithText("last updated", substring = true).performScrollTo().assertIsDisplayed()
        rule.onNodeWithText("No account has reported usage yet.").assertDoesNotExist()

        rule.onNodeWithContentDescription("Usage").performClick()
        rule.onNodeWithContentDescription("Usage").performClick()
        rule.waitUntil { requests.get() == 2 }
        rule.onNodeWithText("OpenRouter: Work key").performScrollTo().assertIsDisplayed()
        rule.onNodeWithText("$12.35").assertIsDisplayed()
    }

    @Test
    fun theUsageButtonTogglesACardOfAccountsWithUsageByDomainWithTheOpenChatsAccountFirst() {
        showShell()

        rule.onNodeWithText("This chat").assertDoesNotExist()

        rule.onNodeWithContentDescription("Usage").performClick()

        rule.onNodeWithText("This chat").assertIsDisplayed()
        rule.onNodeWithText("40% used", substring = true).assertIsDisplayed()

        val thisChatTop = rule.onNodeWithText("This chat").getUnclippedBoundsInRoot().top
        val annTop = rule.onNodeWithText("example.net").getUnclippedBoundsInRoot().top

        assertTrue(thisChatTop < annTop)
        rule.onNodeWithText("ann@example.net").assertDoesNotExist()
        rule.onNodeWithText("example.org").assertDoesNotExist()
        rule.onNodeWithText("Usage unknown").assertDoesNotExist()

        rule.onNodeWithContentDescription("Usage").performClick()

        rule.onNodeWithText("This chat").assertDoesNotExist()
    }
}

private fun Float.toDp(rule: ComposeContentTestRule): Dp = with(rule.density) { this@toDp.toDp() }

private fun assertEquals(expected: Dp, actual: Dp, tolerance: Dp) {
    assertTrue((expected - actual).value.let { kotlin.math.abs(it) } <= tolerance.value, "expected $expected, was $actual")
}
