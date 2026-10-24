package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
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
import mycelium.api.ModelProviderUsage
import mycelium.api.OpenRouterSpend
import mycelium.ui.UsageCard
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
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
    fun anUnusedKeyShowsZeroSpendAndUnreportedPeriodsStayUnknown() {
        rule.setContent {
            MyceliumTheme(true) {
                UsageCard(emptyList(), providers = listOf(ModelProviderUsage("p1", "Unused key", spend = OpenRouterSpend(total = 0.0))))
            }
        }

        rule.onNodeWithText("$0.00").assertIsDisplayed()
        rule.onAllNodes(hasText("Not reported")).fetchSemanticsNodes().let { assertEquals(3, it.size) }
        rule.onNodeWithText("Key limit remaining").assertDoesNotExist()
        rule.onAllNodes(hasText("Last updated", substring = true)).fetchSemanticsNodes().let { assertEquals(0, it.size) }
    }

    @Test
    fun openRouterKeySpendAppearsAlongsideAccountUsage() {
        val requests = showShell(openRouter = true, failAfterFirst = true)
        assertEquals(0, requests.get())
        rule.onNodeWithContentDescription("Usage").performClick()
        rule.waitUntil { rule.onAllNodes(hasText("OpenRouter: Work key")).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Key spend (USD)").assertIsDisplayed()
        rule.onNodeWithText("$12.35").assertIsDisplayed()
        rule.onNodeWithText("$0.13").assertIsDisplayed()
        rule.onNodeWithText("$87.66").assertIsDisplayed()
        rule.onNodeWithText("Last updated 2099-01-01T10:00:00Z").performScrollTo().assertIsDisplayed()
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
        rule.onNodeWithText("40% used").assertIsDisplayed()

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
