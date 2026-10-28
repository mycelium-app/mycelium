package mycelium.desktop

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import mycelium.api.Me
import mycelium.ui.MyceliumTheme
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class AppsSidebarTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = mutableListOf<String>()

    private fun show(accounts: String) {
        val store = appsStore(scopes.create()) { request ->
            val path = request.url.encodedPath
            requests += path

            when {
                path.endsWith("/chat-tool-servers") -> json(accounts)
                path.endsWith("/nodes") || path.endsWith("/conversations") -> json("[]")
                path.endsWith("/apps") -> json("""{"apps":[]}""")
                else -> json("{}")
            }
        }

        rule.setContent { MyceliumTheme(true) { Shell(store, AppsPlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }
    }

    @Test
    fun theAppsLinkAppearsOnceACoolifyAccountExists() {
        show("[$CoolifyAccount]")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Apps").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theAppsLinkIsAbsentWithoutACoolifyAccount() {
        show("[]")

        rule.waitUntil(5_000) { requests.any { it.endsWith("/chat-tool-servers") } }
        rule.waitForIdle()
        assertEquals(0, rule.onAllNodesWithText("Apps").fetchSemanticsNodes().size)
    }

    @Test
    fun openingAppsWithNoAppsExplainsAndOffersANewConversation() {
        show("[$CoolifyAccount]")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Apps").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Apps").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("No apps yet").fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText("New conversation").assertCountEquals(2)
    }
}
