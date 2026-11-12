package mycelium.desktop

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
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
import mycelium.api.ResetCredit
import mycelium.api.ResetCredits
import mycelium.api.UsageWindow
import mycelium.state.AgentAccount
import mycelium.state.Store
import mycelium.ui.AccountsSettings
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.ui.Modifier
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.UsageCard
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val RecordedNodes = """[{"id":"n1","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"v0.11.0","address":"10.0.0.5","online":true,"draining":false,"sessions":[],
"agents":[{"agent":"claude","available":true},{"agent":"codex","available":true}],
"logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in","windows":[{"name":"five_hour","used_percent":82.0}],
 "reset_credits":{"available_count":2,"credits":[{"id":"r1","expires_at":"2099-11-13T23:59:59Z","title":"Fable reset","manual":true},{"id":"r2","expires_at":"2099-11-27T23:59:59Z","title":"Full reset","manual":true}]}},
 {"agent":"codex","account":"alex@example.org","method":"token","state":"signed_in"}]}]"""

private val RecordedCredits = ResetCredits(2, listOf(ResetCredit("r1", "2099-11-13T23:59:59Z", "Fable reset", manual = true), ResetCredit("r2", "2099-11-27T23:59:59Z", "Full reset", manual = true)))

class ClaudeRecordedResetsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val sent = CopyOnWriteArrayList<Pair<String, String>>()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method != HttpMethod.Get) {
                sent += "${request.method.value} $path" to String(request.body.toByteArray())

                return@MockEngine respond(if (request.method == HttpMethod.Delete) "" else """{"id":"r3","title":"Full reset","manual":true}""", if (request.method == HttpMethod.Delete) HttpStatusCode.NoContent else HttpStatusCode.Created, json)
            }

            val body = when {
                path.endsWith("/nodes") -> RecordedNodes
                path.endsWith("/conversations") || path.endsWith("/model-providers") || path.endsWith("/account-prices") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        return store
    }

    private fun claudeWithRecordedResets() = AgentAccount("claude", "sam@example.com", "token", emptyList(), windows = listOf(UsageWindow("five_hour", 82.0)), resetCredits = RecordedCredits)

    private fun openAddReset() {
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store()) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for sam@example.com").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for sam@example.com").performClick()
        rule.onNodeWithText("Add a reset").performClick()
    }

    private fun problemsAfterAdding(): Boolean {
        rule.onAllNodesWithText("Add reset").onLast().performClick()

        return rule.onAllNodesWithText("There is a problem").fetchSemanticsNodes().isNotEmpty()
    }

    @Test
    fun aClaudeAccountShowsItsRecordedResetsWithTheSoonestExpiry() {
        rule.setContent { MyceliumTheme(true) { UsageCard(listOf(claudeWithRecordedResets() to false), onUseReset = { _, _, _ -> "marked" }) } }

        rule.onNodeWithText("2 banked, expires 13 Nov").assertIsDisplayed()
        rule.onNodeWithContentDescription("Use a banked reset for sam@example.com").assertIsDisplayed()
        rule.captureScreen("usage-card-recorded-resets")
    }

    @Test
    fun usingARecordedResetSaysItIsMarkedUsedInMyceliumAndSendsTheChosenReset() {
        val calls = CopyOnWriteArrayList<String?>()

        rule.setContent { MyceliumTheme(true) { UsageCard(listOf(claudeWithRecordedResets() to false), onUseReset = { _, _, creditId -> calls.add(creditId); "marked" }) } }
        rule.onNodeWithText("Use reset").performClick()

        rule.onNodeWithText("This marks the Fable reset as used in Mycelium. Use it in Claude's settings too.").assertIsDisplayed()
        rule.captureScreen("use-recorded-reset")
        rule.onNodeWithText("Full reset, expires 27 Nov").performClick()
        rule.onNodeWithText("This marks the Full reset as used in Mycelium. Use it in Claude's settings too.").assertIsDisplayed()
        rule.onNodeWithText("Mark as used").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Marked as used.").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(listOf<String?>("r2"), calls.toList())
    }

    @Test
    fun aSingleRecordedResetIsMarkedWithoutAskingWhichOne() {
        val credits = ResetCredits(1, RecordedCredits.credits!!.take(1))
        val account = claudeWithRecordedResets().copy(resetCredits = credits)

        rule.setContent { MyceliumTheme(true) { UsageCard(listOf(account to false), onUseReset = { _, _, _ -> "marked" }) } }
        rule.onNodeWithText("Use reset").performClick()

        rule.onAllNodesWithText("Which reset").assertCountEquals(0)
        rule.onNodeWithText("Mark as used").assertIsDisplayed()
    }

    @Test
    fun theProvidersPageListsRecordedResetsAndRemoveDeletesOne() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Box(Modifier.background(MyTheme.colors.bgPanel)) { AccountsSettings(store) } } }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Limit resets").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Expires 13 Nov").assertIsDisplayed()
        rule.captureScreen("providers-recorded-resets")
        rule.onNodeWithContentDescription("Remove the Fable reset, expires 13 Nov").performClick()

        rule.waitUntil(5_000) { sent.any { it.first == "DELETE /api/v1/usage/manual-resets/r1" } }
    }

    @Test
    fun onlyClaudeAccountsOfferToAddAReset() {
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store()) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for alex@example.org").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for alex@example.org").performClick()

        rule.onNodeWithText("Set your price").assertIsDisplayed()
        rule.onAllNodesWithText("Add a reset").assertCountEquals(0)
    }

    @Test
    fun addingAResetWithNothingFilledInNamesWhatToFix() {
        openAddReset()
        rule.captureScreen("add-reset")

        assertTrue(problemsAfterAdding())
        rule.onAllNodesWithText("Select a kind of reset").let { assertTrue(it.fetchSemanticsNodes().isNotEmpty()) }
        rule.onAllNodesWithText("Enter the date the reset expires").let { assertTrue(it.fetchSemanticsNodes().isNotEmpty()) }
        rule.captureScreen("add-reset-error")
        assertEquals(emptyList(), sent.toList())
    }

    @Test
    fun aModelResetNeedsAModel() {
        openAddReset()
        rule.onNodeWithText("Model reset").performClick()
        rule.onNodeWithContentDescription("Day").performTextInput("27")
        rule.onNodeWithContentDescription("Month").performTextInput("11")
        rule.onNodeWithContentDescription("Year").performTextInput("2099")

        assertTrue(problemsAfterAdding())
        rule.onAllNodesWithText("Enter the model the reset is for").let { assertTrue(it.fetchSemanticsNodes().isNotEmpty()) }
        assertEquals(emptyList(), sent.toList())
    }

    @Test
    fun anIncompleteDateSaysWhichPartIsMissing() {
        openAddReset()
        rule.onAllNodesWithText("Full reset").onLast().performClick()
        rule.onNodeWithContentDescription("Day").performTextInput("27")
        rule.onNodeWithContentDescription("Year").performTextInput("2099")

        assertTrue(problemsAfterAdding())
        rule.onAllNodesWithText("Expiry date must include a month").let { assertTrue(it.fetchSemanticsNodes().isNotEmpty()) }
    }

    @Test
    fun aDateThatDoesNotExistIsRefused() {
        openAddReset()
        rule.onAllNodesWithText("Full reset").onLast().performClick()
        rule.onNodeWithContentDescription("Day").performTextInput("31")
        rule.onNodeWithContentDescription("Month").performTextInput("2")
        rule.onNodeWithContentDescription("Year").performTextInput("2099")

        assertTrue(problemsAfterAdding())
        rule.onAllNodesWithText("Enter a real date, like 27 11 2026").let { assertTrue(it.fetchSemanticsNodes().isNotEmpty()) }
    }

    @Test
    fun aDateInThePastIsRefused() {
        openAddReset()
        rule.onAllNodesWithText("Full reset").onLast().performClick()
        rule.onNodeWithContentDescription("Day").performTextInput("2")
        rule.onNodeWithContentDescription("Month").performTextInput("11")
        rule.onNodeWithContentDescription("Year").performTextInput("2020")

        assertTrue(problemsAfterAdding())
        rule.onAllNodesWithText("Expiry date must be in the future").let { assertTrue(it.fetchSemanticsNodes().isNotEmpty()) }
    }

    @Test
    fun aModelResetWithAFutureDateIsSentAsADay() {
        openAddReset()
        rule.onNodeWithText("Model reset").performClick()
        rule.onNode(hasContentDescription("Model", substring = true)).performTextInput("Fable")
        rule.onNodeWithContentDescription("Day").performTextInput("27")
        rule.onNodeWithContentDescription("Month").performTextInput("11")
        rule.onNodeWithContentDescription("Year").performTextInput("2099")
        rule.captureScreen("add-reset-filled")
        rule.onAllNodesWithText("Add reset").onLast().performClick()

        rule.waitUntil(5_000) { sent.any { it.first == "POST /api/v1/usage/manual-resets" } }
        assertEquals("""{"agent":"claude","account":"sam@example.com","kind":"model","model":"Fable","expires_on":"2099-11-27"}""", sent.first { it.first == "POST /api/v1/usage/manual-resets" }.second)
    }
}
