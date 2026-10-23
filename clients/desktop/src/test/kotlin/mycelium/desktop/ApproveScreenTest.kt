package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.api.MyceliumApi
import mycelium.ui.ApproveScreen
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class ApproveScreenTest {
    @get:Rule
    val rule = createComposeRule()

    private var closed = 0

    private fun show(kind: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val body = if (request.url.encodedPath.endsWith("/approve")) "{}" else """{"user_code":"ABCD-1234","kind":"$kind","label":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","ip":"192.0.2.1","expires_at":"2026-10-23T10:00:00Z"}"""

            respond(body, HttpStatusCode.OK, json)
        }
        val api = MyceliumApi("https://mycelium.test", "token", engine)
        rule.setContent { MyceliumTheme(true) { ApproveScreen(api, "ABCD-1234", null, onReturn = {}, onDone = {}, onClose = { closed++ }) } }
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Check this code matches the one on the machine").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun approvingTheDesktopAppTriesToCloseTheTabAndSaysItCanBeClosed() {
        show("client")
        rule.onNodeWithText("Approve laptop (desktop app)").performClick()
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("You can close this tab and return to Mycelium.").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(1, closed)
    }

    @Test
    fun approvingANodeLeavesTheTabAlone() {
        show("node")
        rule.onNodeWithText("Approve laptop").performClick()
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Approved. You can close this page and go back to the machine.").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(0, closed)
    }
}
