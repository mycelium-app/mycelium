package mycelium

import mycelium.ui.firstUrl
import mycelium.ui.oneTimeCode
import mycelium.ui.withoutTerminalCodes
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

private const val CodexDeviceSignIn = "\r\nWelcome to Codex [v\u001B[90m0.164.0\u001B[0m]\r\n" +
    "1. Open this link in your browser and sign in to your account\r\n" +
    "   \u001B[94mhttps://auth.openai.com/codex/device\u001B[0m\r\n\r\n" +
    "2. Enter this one-time code \u001B[90m(expires in 15 minutes)\u001B[0m\r\n" +
    "   \u001B[94mAB12-CD34E\u001B[0m\r\n"

private const val ClaudeAuthorizeUrl = "https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c250a&response_type=code" +
    "&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope=user%3Aprofile+user%3Ainference&code_challenge=Oy3dM3&state=a5uC-hQf"

private const val ClaudeAuthLogin = "Opening browser to sign in\u2026\r\nIf the browser didn't open, visit: " +
    "\u001B]8;;$ClaudeAuthorizeUrl\u0007$ClaudeAuthorizeUrl\u001B]8;;\u0007\r\nPaste code here if prompted > "

class LoginOutputTest {
    @Test
    fun codexSignInOutputShowsWithoutColourCodesAndGivesTheCleanLinkAndCode() {
        val text = withoutTerminalCodes(CodexDeviceSignIn)

        assertEquals("\nWelcome to Codex [v0.164.0]\n", text.lines().take(2).joinToString("\n", postfix = "\n"))
        assertEquals("https://auth.openai.com/codex/device", firstUrl(text))
        assertEquals("AB12-CD34E", oneTimeCode(text))
    }

    @Test
    fun claudeSignInOutputHasALinkButNoOneTimeCode() {
        val text = withoutTerminalCodes("Browser didn't open? Use the url below to sign in:\r\n\u001B[4mhttps://claude.ai/oauth/authorize?code=true&client_id=abc\u001B[24m\r\n")

        assertEquals("https://claude.ai/oauth/authorize?code=true&client_id=abc", firstUrl(text))
        assertNull(oneTimeCode(text))
    }

    @Test
    fun claudeAuthLoginOutputGivesTheWholeLinkInsideItsTerminalHyperlink() {
        assertEquals(ClaudeAuthorizeUrl, firstUrl(withoutTerminalCodes(ClaudeAuthLogin)))
    }
}
