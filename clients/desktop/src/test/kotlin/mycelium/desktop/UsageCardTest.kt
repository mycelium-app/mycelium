package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

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
 {"agent":"claude","account":"ann@example.com","method":"oauth_token","state":"signed_in"},
 {"agent":"claude","account":"work@example.com","method":"oauth_token","state":"signed_in","usage_read_at":"2099-01-01T10:00:00Z",
  "windows":[{"name":"five_hour","used_percent":40.0,"resets_at":"2099-01-01T22:30:00Z"}]}],
"sessions":[]}]"""

private const val UsageCardConversations = """[{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Open chat","status":"done","account":"work@example.com",
"acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}]"""

class UsageCardTest {
    @get:Rule
    val rule = createComposeRule()

    private fun showShell() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/nodes") -> UsageCardNodes
                path.endsWith("/conversations") -> UsageCardConversations
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, UsageCardPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    @Test
    fun theUsageButtonTogglesACardWithTheOpenChatsAccountFirst() {
        showShell()

        rule.onNodeWithText("This chat").assertDoesNotExist()

        rule.onNodeWithContentDescription("Usage").performClick()

        rule.onNodeWithText("This chat").assertIsDisplayed()
        rule.onNodeWithText("40% used").assertIsDisplayed()

        val thisChatTop = rule.onNodeWithText("This chat").getUnclippedBoundsInRoot().top
        val annTop = rule.onNodeWithText("ann@example.com").getUnclippedBoundsInRoot().top

        assertTrue(thisChatTop < annTop)

        rule.onNodeWithContentDescription("Usage").performClick()

        rule.onNodeWithText("This chat").assertDoesNotExist()
    }
}
