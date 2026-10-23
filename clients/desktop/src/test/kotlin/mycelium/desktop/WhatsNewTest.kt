package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.state.LastLaunchedVersionPreference
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.RecentReleasesDialog
import mycelium.ui.WhatsNewOnLaunch
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

private class PreferencePlatform : Platform {
    val saved = mutableMapOf<String, String>()

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
}

private val changelog = """
## v0.4.0

- Artifacts arrive.

## v0.3.13

- Close session arrives.

## v0.3.12

- Pinned cards arrive.

## v0.3.9

- Already seen.
""".trimIndent()

class WhatsNewTest {
    @get:Rule
    val rule = createComposeRule()

    private val platform = PreferencePlatform()

    private fun launch(running: String) {
        rule.setContent {
            CompositionLocalProvider(LocalPlatform provides platform) {
                MyceliumTheme(true) {
                    Box(Modifier.height(900.dp)) { WhatsNewOnLaunch(platform, running, changelog) }
                }
            }
        }
    }

    @Test
    fun anOlderStoredVersionShowsTheNewerReleasesAndClosingStoresTheCurrentOne() {
        platform.saved[LastLaunchedVersionPreference] = "0.3.9"
        launch("0.4.0")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Artifacts arrive.").fetchSemanticsNodes().size == 1 }
        rule.onNodeWithText("Close session arrives.").assertExists()
        rule.onNodeWithText("Pinned cards arrive.").assertExists()
        rule.onNodeWithText("Already seen.").assertDoesNotExist()
        assertEquals("0.3.9", platform.saved[LastLaunchedVersionPreference])

        rule.onNodeWithText("Close").performClick()
        rule.waitUntil(5_000) { platform.saved[LastLaunchedVersionPreference] == "0.4.0" }

        rule.onNodeWithText("What's new").assertDoesNotExist()
    }

    @Test
    fun aFirstLaunchStoresTheVersionWithoutShowingAnything() {
        launch("0.4.0")

        rule.waitUntil(5_000) { platform.saved[LastLaunchedVersionPreference] == "0.4.0" }

        rule.onNodeWithText("What's new").assertDoesNotExist()
    }

    @Test
    fun aDevelopmentBuildStoresNothing() {
        platform.saved[LastLaunchedVersionPreference] = "0.3.9"
        launch("dev")
        rule.waitForIdle()

        rule.onNodeWithText("What's new").assertDoesNotExist()
        assertEquals("0.3.9", platform.saved[LastLaunchedVersionPreference])
    }

    @Test
    fun theSettingsEntryShowsTheLastFiveReleasesWithoutStoringAnything() {
        rule.setContent {
            MyceliumTheme(true) { RecentReleasesDialog(onClose = {}, changelog = changelog) }
        }

        rule.onNodeWithText("Already seen.").assertExists()
        rule.onNodeWithText("Artifacts arrive.").assertExists()
        assertNull(platform.saved[LastLaunchedVersionPreference])
    }
}
