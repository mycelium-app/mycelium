package mycelium

import mycelium.ui.loginMethodFor
import kotlin.test.Test
import kotlin.test.assertEquals

class LoginMethodTest {
    @Test
    fun signInAsksTheNodeForTheMethodNamesItAccepts() {
        assertEquals("oauth_token", loginMethodFor("claude"))
        assertEquals("device_auth", loginMethodFor("codex"))
    }
}
