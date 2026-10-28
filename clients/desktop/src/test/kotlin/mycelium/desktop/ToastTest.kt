package mycelium.desktop

import androidx.compose.foundation.layout.Column
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.ApiException
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.BreakClock
import mycelium.state.BreakPhase
import mycelium.state.Store
import mycelium.ui.BreakToast
import mycelium.ui.ErrorToast
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class ToastTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val failure = "The control plane could not reach the node."

    private fun store(): Store {
        val engine = MockEngine { respond("[]", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json")) }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        return store
    }

    private fun toastCount() = rule.onAllNodesWithContentDescription("Error: $failure").fetchSemanticsNodes().size

    @Test
    fun aFailedActionShowsAnErrorToastWithRetryAndDismissRemovesIt() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, HandsOffPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Empty) } }

        store.load { throw ApiException(502, "unreachable", failure) }

        rule.waitUntil(5_000) { toastCount() > 0 }
        rule.onNodeWithContentDescription("Error: $failure").assertIsDisplayed()
        rule.onNodeWithText("Retry").assertIsDisplayed()

        rule.onNodeWithContentDescription("Dismiss").performClick()

        rule.waitUntil(5_000) { toastCount() == 0 }
    }

    @Test
    fun theErrorAndTheBreakReminderShowTogether() {
        val taken = mutableListOf<String>()
        rule.setContent {
            MyceliumTheme(true) {
                Column {
                    ErrorToast(failure, onDismiss = {}, onRetry = { taken += "retry" })
                    BreakToast(BreakClock(BreakPhase.Due, activeMinutes = 50), 5, { taken += "snooze" }, { taken += "break" }, {})
                }
            }
        }

        rule.onNodeWithText("Retry").performClick()
        rule.onNodeWithText("Start break").performClick()
        rule.onNodeWithText("Snooze").performClick()

        assertEquals(listOf("retry", "break", "snooze"), taken)
    }
}
