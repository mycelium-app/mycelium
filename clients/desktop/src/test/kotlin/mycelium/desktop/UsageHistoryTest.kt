package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import org.jetbrains.skia.EncodedImageFormat
import org.jetbrains.skia.Image
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.ui.UsageCard
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

internal const val HistoryAccounts = """{"accounts":[
{"agent":"claude","account":"me@example.com","plan":"max_20x","plan_price_usd":200.0,"plan_price_as_of":"2026-10-27","readings_since":"2026-09-03T08:00:00Z",
"readings":[{"at":"2026-10-26T08:00:00Z","window":"five_hour","percent":4.0},{"at":"2026-10-26T10:00:00Z","window":"five_hour","percent":61.0},{"at":"2026-10-26T08:00:00Z","window":"seven_day","percent":20.0},{"at":"2026-10-26T10:00:00Z","window":"seven_day","percent":31.0}],
"windows":[{"window":"five_hour","started_at":"2026-10-26T08:00:00Z","peak_percent":61.0,"tokens":118000000,"tokens_per_point":1900000.0,"api_equivalent_usd":52.0}],
"tokens_per_point":{"five_hour":1900000.0,"seven_day":22000000.0,"five_hour_samples":41,"seven_day_samples":7},
"api_equivalent_month_usd":1310.0,"api_equivalent_per_window_usd":46.0},
{"agent":"codex","account":"sam@example.org","readings":[],"windows":[],"tokens_per_point":{"five_hour_samples":0,"seven_day_samples":0}}]}"""

class UsageHistoryTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val asked = CopyOnWriteArrayList<String>()

    private fun open(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val body = when {
                request.url.encodedPath.endsWith("/usage/history") -> {
                    asked += request.url.encodedQuery
                    HistoryAccounts
                }
                request.url.encodedPath.endsWith("/nodes") || request.url.encodedPath.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.UsageHistory) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Max 20x").fetchSemanticsNodes().isNotEmpty() }

        return store
    }

    @Test
    fun eachAccountShowsItsPlanTokensPerPercentageApiEquivalentAndSubscription() {
        open()

        rule.onNodeWithText("example.com").assertExists()
        rule.onNodeWithText("Max 20x").assertExists()
        rule.onAllNodesWithText("~1.9M")[0].assertExists()
        rule.onNodeWithText("median of 41 windows").assertExists()
        rule.onNodeWithText("median of 7 weeks").assertExists()
        rule.onNodeWithText("~$1,310 this month").assertExists()
        rule.onNodeWithText("~$46 per 5-hour window").assertExists()
        rule.onNodeWithText("$200 a month").assertExists()
        rule.onNodeWithText("API equivalent is 6.6 times the price").assertExists()
    }

    @Test
    fun theScreenRendersForComparisonWithTheArtboard() {
        open()
        val png = Image.makeFromBitmap(rule.onRoot().captureToImage().asSkiaBitmap()).encodeToData(EncodedImageFormat.PNG)!!.bytes
        val out = java.io.File("build/screens/usage-history-dark.png")
        out.parentFile.mkdirs()
        out.writeBytes(png)

        assertEquals(true, out.length() > 5_000)
    }

    @Test
    fun anAccountWithoutAReportedPlanSaysThePriceIsUnknown() {
        open()

        rule.onNodeWithText("Plan price unknown").assertExists()
    }

    @Test
    fun theFiveHourWindowsTableListsEachWindowWithItsUseTokensAndValue() {
        open()

        rule.onAllNodesWithText("5-hour windows")[0].assertExists()
        rule.onNodeWithText("61%").assertExists()
        rule.onNodeWithText("118M").assertExists()
        rule.onNodeWithText("~$52").assertExists()
    }

    @Test
    fun choosingAPeriodAndAnAccountAsksAgainWithThem() {
        open()

        rule.onNodeWithText("30 days").performClick()
        rule.waitUntil(5_000) { asked.last() == "range=30d" }

        rule.onNodeWithContentDescription("Choose accounts").performClick()
        rule.onAllNodesWithText("example.org")[1].performClick()
        rule.waitUntil(5_000) { asked.last() == "range=30d&account=codex%3Asam%40example.org" }
    }

    @Test
    fun allTimeIsOfferedAsAPeriod() {
        open()

        rule.onNodeWithText("All time").performClick()
        rule.waitForIdle()

        assertEquals("range=all", asked.last())
    }

    @Test
    fun theUsageCardOffersTheHistoryAsAnArrowNextToItsTitle() {
        var opened = 0
        rule.setContent { MyceliumTheme(true) { UsageCard(emptyList(), onOpenHistory = { opened++ }) } }

        rule.onNodeWithContentDescription("Usage history").performClick()

        assertEquals(1, opened)
    }
}
