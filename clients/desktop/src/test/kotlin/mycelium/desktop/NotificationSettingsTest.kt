package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.NotifyFinishedPreference
import mycelium.state.NotifySoundPreference
import mycelium.state.Store
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.SettingsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private class SwitchPlatform : Platform {
    val saved = mutableMapOf<String, String>()
    var permissionAsked = 0

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
    override fun loadPreference(key: String): String? = saved[key]
    override fun savePreference(key: String, value: String) {
        saved[key] = value
    }
    override fun requestNotificationPermission() {
        permissionAsked++
    }
}

class NotificationSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    private val platform = SwitchPlatform()

    private fun show() {
        val engine = MockEngine { respond("[]", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json")) }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        rule.setContent {
            CompositionLocalProvider(LocalPlatform provides platform) {
                MyceliumTheme(true) {
                    Box(Modifier.height(900.dp)) { SettingsScreen(store, Me("u", "sam@example.com", "Sam"), "https://mycelium.test", onSignOut = {}) }
                }
            }
        }
    }

    private fun state(value: String) = SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, value)

    @Test
    fun bothSwitchesStartOn() {
        show()

        rule.onNodeWithContentDescription("Notify when an agent finishes").performScrollTo().assert(state("On"))
        rule.onNodeWithContentDescription("Play a sound").performScrollTo().assert(state("On"))
    }

    @Test
    fun turningTheSoundOffIsSavedOnThisDevice() {
        show()

        rule.onNodeWithContentDescription("Play a sound").performScrollTo().performClick()
        rule.waitUntil(5_000) { platform.saved[NotifySoundPreference] == "false" }

        rule.onNodeWithContentDescription("Play a sound").assert(state("Off"))
    }

    @Test
    fun turningNotificationsBackOnAsksForPermission() {
        show()

        rule.onNodeWithContentDescription("Notify when an agent finishes").performScrollTo().performClick()
        rule.waitUntil(5_000) { platform.saved[NotifyFinishedPreference] == "false" }
        assertEquals(0, platform.permissionAsked)

        rule.onNodeWithContentDescription("Notify when an agent finishes").performClick()
        rule.waitUntil(5_000) { platform.permissionAsked == 1 }
    }
}
