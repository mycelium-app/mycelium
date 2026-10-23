package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.AccountsSettings
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val UsageNodes = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"0.1.0","address":"100.64.4.18","online":true,"draining":false,
 "agents":[{"agent":"claude","version":"2.1.308","available":true},{"agent":"codex","version":"0.163.0","available":true}],
 "logins":[
  {"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"signed_in","usage_left":18,"usage_read_at":"2099-01-01T10:00:00Z",
   "windows":[
    {"name":"five_hour","used_percent":82.0,"resets_at":"2099-01-01T22:30:00Z","rate_per_hour":27.0,"runs_out_at":"2099-01-01T18:40:00Z"},
    {"name":"seven_day","used_percent":31.0,"resets_at":"2099-01-04T09:00:00Z"}]},
  {"agent":"claude","account":"work@acme.example","method":"oauth_token","state":"signed_in"},
  {"agent":"codex","account":"sam@example.com","method":"device_auth","state":"signed_in"}]}]"""

private object ZonedPlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection() = null
    override fun saveConnection(connection: mycelium.Connection?) = Unit
    override fun device() = mycelium.DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
    override fun localDateTime(epochMillis: Long): String? = java.time.LocalDateTime.ofInstant(java.time.Instant.ofEpochMilli(epochMillis), java.time.ZoneOffset.ofHours(1)).format(java.time.format.DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm"))
}

class AccountUsageTest {
    @get:Rule
    val rule = createComposeRule()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request -> if (request.url.encodedPath.endsWith("/nodes")) respond(UsageNodes, HttpStatusCode.OK, json) else respond("[]", HttpStatusCode.OK, json) }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        runBlocking { store.refresh() }

        return store
    }

    @Test
    fun eachWindowShowsAsABarWithItsPercentUsedItsResetInLocalTimeAndTheProjection() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { CompositionLocalProvider(LocalPlatform provides ZonedPlatform) { AccountsSettings(store) } } }

        rule.onNodeWithContentDescription("5-hour window 82% used").assertExists()
        rule.onNodeWithText("82% used").assertExists()
        rule.onNodeWithText("At this rate, runs out about 1 Jan 19:40, resets 1 Jan 23:30").assertExists()
        rule.onNodeWithContentDescription("Weekly window 31% used").assertExists()
        rule.onNodeWithText("Resets 4 Jan 10:00").assertExists()
    }

    @Test
    fun anAccountWithoutWindowsSaysItsUsageIsUnknown() {
        val store = store()
        rule.setContent { MyceliumTheme(true) { AccountsSettings(store) } }

        assertEquals(2, rule.onAllNodesWithText("Usage unknown").fetchSemanticsNodes().size)
    }
}
