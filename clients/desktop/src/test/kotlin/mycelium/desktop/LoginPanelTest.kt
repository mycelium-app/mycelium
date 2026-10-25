package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import mycelium.ui.LoginPanel
import mycelium.ui.LoginSession
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class LoginPanelTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun sendingThePastedCodePressesEnterInTheSignInTerminal() {
        val sent = mutableListOf<String>()
        rule.setContent {
            MyceliumTheme {
                LoginPanel(LoginSession("l1", "claude"), "Paste code here if prompted > ", null, {}, {}, { sent += it }, {})
            }
        }

        rule.onNodeWithContentDescription("Reply to the sign-in").performTextInput("abc123#state")
        rule.onNodeWithText("Send").performClick()
        rule.waitForIdle()

        assertEquals(listOf("abc123#state\r"), sent)
    }
}
