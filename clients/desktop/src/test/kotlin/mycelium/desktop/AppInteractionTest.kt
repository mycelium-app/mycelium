package mycelium.desktop

import androidx.compose.ui.test.click
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performMouseInput
import mycelium.App
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.ui.MyceliumTheme
import mycelium.ui.SignInScreen
import org.junit.Rule
import kotlin.math.abs
import kotlin.test.Test
import kotlin.test.assertTrue

private class StubPlatform(override val signsInThroughTheBrowser: Boolean) : Platform {
    override val originUrl: String? = null
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

class AppInteractionTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun aMouseClickOnAButtonRunsIt() {
        rule.setContent { App(StubPlatform(signsInThroughTheBrowser = false), dark = true) }

        rule.onNodeWithText("Continue").performMouseInput { click() }

        rule.onNodeWithText("Enter the address of your control plane", substring = true).assertExists()
    }

    @Test
    fun aTapOnAButtonRunsIt() {
        rule.setContent { App(StubPlatform(signsInThroughTheBrowser = false), dark = true) }

        rule.onNodeWithText("Continue").performClick()

        rule.onNodeWithText("Enter the address of your control plane", substring = true).assertExists()
    }


    @Test
    fun theBrowserSignInButtonIsCentredUnderTheTitle() {
        rule.setContent { MyceliumTheme(true) { SignInScreen(StubPlatform(signsInThroughTheBrowser = true), null, null) { _, _ -> } } }

        val title = rule.onNodeWithText("Mycelium").getBoundsInRoot()
        val button = rule.onNodeWithText("Sign in").getBoundsInRoot()
        val titleCentre = (title.left + title.right) / 2
        val buttonCentre = (button.left + button.right) / 2

        assertTrue(abs((titleCentre - buttonCentre).value) < 1f, "title centre $titleCentre, button centre $buttonCentre")
    }
}
