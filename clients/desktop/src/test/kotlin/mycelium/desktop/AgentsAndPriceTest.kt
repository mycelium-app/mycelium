package mycelium.desktop

import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.AccountsSettings
import mycelium.ui.AgentsSettings
import mycelium.ui.MyceliumTheme
import mycelium.ui.UsageHistoryScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val AccountNodes = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.11.0","address":"10.0.0.5","online":true,"draining":false,"sessions":[],
"agents":[{"agent":"claude","available":true},{"agent":"codex","available":true}],
"logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in"},{"agent":"claude","account":"work@example.org","method":"token","state":"signed_in"},{"agent":"codex","account":"codex@example.com","method":"token","state":"signed_in"}]}]"""

private const val NoSettings = """{"default_modes":{},"account_modes":[],"any_model":[]}"""

private const val History = """{"accounts":[{"agent":"claude","account":"sam@example.com","plan":"max_20x","plan_price_usd":200.0,"plan_price_as_of":"2026-10-27",
"your_price":{"amount":1800.0,"currency":"GBP","period":"yearly","monthly_amount":150.0,"monthly_usd":195.0,"rate_date":"2026-10-27"},
"readings":[],"windows":[],"tokens_per_point":{"five_hour_samples":0,"seven_day_samples":0},"api_equivalent_month_usd":1310.0}]}"""

class AgentsAndPriceTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val sent = CopyOnWriteArrayList<Pair<String, String>>()

    private fun store(settings: String = NoSettings, history: String = History): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method != HttpMethod.Get) {
                sent += "${request.method.value} $path" to String(request.body.toByteArray())

                return@MockEngine respond(if (path.endsWith("/agent-settings")) settings else "", if (path.endsWith("/agent-settings")) HttpStatusCode.OK else HttpStatusCode.NoContent, json)
            }

            val body = when {
                path.endsWith("/nodes") -> AccountNodes
                path.endsWith("/agent-settings") -> settings
                path.endsWith("/usage-limit") -> """{"on_usage_limit":"resume"}"""
                path.endsWith("/usage/history") -> history
                path.endsWith("/conversations") || path.endsWith("/model-providers") || path.endsWith("/account-prices") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        return store
    }

    private fun body(call: String): String = sent.toList().last { it.first == call }.second

    private fun waitFor(call: String) = rule.waitUntil(5_000) { sent.toList().any { it.first == call } }

    @Test
    fun aDefaultModeIsSavedAtOnceUnderTheHarnessItWasChosenFor() {
        val store = store(settings = """{"default_modes":{"claude":"plan"},"account_modes":[],"any_model":[]}""")
        rule.setContent { MyceliumTheme(true) { AgentsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Default mode").fetchSemanticsNodes().isNotEmpty() }
        rule.onNode(hasText("Auto") and hasAnyAncestor(hasContentDescription("Default mode for Codex"))).performClick()

        waitFor("PUT /api/v1/agent-settings")
        assertEquals("""{"default_modes":{"claude":"plan","codex":"auto"}}""", body("PUT /api/v1/agent-settings"))
    }

    @Test
    fun anAccountsDefaultModeIsChangedFromItsRowMenu() {
        val store = store(settings = """{"default_modes":{"claude":"plan"},"account_modes":[],"any_model":[]}""")
        rule.setContent { MyceliumTheme(true) { AgentsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for work@example.org").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for work@example.org").performClick()
        rule.onNodeWithText("Change default mode").performClick()
        rule.onNodeWithContentDescription("Mode: Same as Claude Code (Plan)").performClick()
        rule.onAllNodesWithText("Bypass").onLast().performClick()
        rule.onNodeWithText("Save").performClick()

        waitFor("PUT /api/v1/agent-settings/account-mode")
        assertEquals("""{"agent":"claude","account":"work@example.org","mode":"bypassPermissions"}""", body("PUT /api/v1/agent-settings/account-mode"))
    }

    @Test
    fun theAnyModelListGainsAnEntryInTheDialogAndIsSavedInOrder() {
        val store = store(settings = """{"default_modes":{},"account_modes":[],"any_model":[{"agent":"claude"}]}""")
        rule.setContent { MyceliumTheme(true) { AgentsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Edit list").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Edit list").performClick()
        rule.onNodeWithContentDescription("Model: Add a favourite").performClick()
        rule.onNodeWithContentDescription("Codex: Codex").performClick()
        rule.onNodeWithText("Save").performClick()

        waitFor("PUT /api/v1/agent-settings")
        assertEquals("""{"any_model":[{"agent":"claude"},{"agent":"codex"}]}""", body("PUT /api/v1/agent-settings"))
    }

    @Test
    fun aPriceWithoutAPlainNumberIsRefusedInPlainWordsAndNothingIsSaved() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for sam@example.com").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for sam@example.com").performClick()
        rule.onNodeWithText("Set your price").performClick()
        rule.onNodeWithContentDescription("Amount").performTextInput("1.800,00")
        rule.onNodeWithText("Save").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Enter the amount as a number, like 1800 or 150.50").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("There is a problem").assertExists()
        assertEquals(emptyList(), sent.filter { it.first == "PUT /api/v1/account-prices" })
    }

    @Test
    fun aYearlyPriceIsSavedForTheAccount() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for sam@example.com").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for sam@example.com").performClick()
        rule.onNodeWithText("Set your price").performClick()
        rule.onNodeWithContentDescription("Amount").performTextInput("1,800")
        rule.onNodeWithText("Yearly").performClick()
        rule.onNodeWithText("Save").performClick()

        waitFor("PUT /api/v1/account-prices")
        assertEquals("""{"agent":"claude","account":"sam@example.com","price":{"amount":1800.0,"currency":"GBP","period":"yearly"}}""", body("PUT /api/v1/account-prices"))
    }

    @Test
    fun usingTheListPriceClearsThePrice() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for sam@example.com").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for sam@example.com").performClick()
        rule.onNodeWithText("Set your price").performClick()
        rule.onNodeWithText("Use the list price").performClick()

        waitFor("PUT /api/v1/account-prices")
        assertEquals("""{"agent":"claude","account":"sam@example.com"}""", body("PUT /api/v1/account-prices"))
    }

    @Test
    fun usageHistoryShowsYourPriceInItsOwnCurrencyBesideTheListPrice() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { UsageHistoryScreen(store) } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("£150 a month").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("£1,800 a year. Max 20x lists $200. API equivalent is 6.7 times this").assertExists()
        rule.onNodeWithText("Change your price").assertExists()
    }
}
