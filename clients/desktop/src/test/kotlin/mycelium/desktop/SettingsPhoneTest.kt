package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
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
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.LocalOpenList
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.SettingsScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class SettingsPhoneTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show(touch: Boolean = true) {
        val engine = MockEngine { respond("[]", HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json")) }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalTouch provides touch, LocalOpenList provides {}) {
                    SettingsScreen(store, Me("u", "sam@example.com", "Sam"), "https://mycelium.test", onSignOut = {})
                }
            }
        }
    }

    @Test
    fun settingsOpensOnAListOfSectionsWithTheDrawerMenuAndNoBackButton() {
        show()

        rule.onNodeWithText("Memories").assertExists()
        rule.onNodeWithText("Instructions").assertExists()
        assertEquals(1, rule.onAllNodesWithContentDescription("Conversations").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Back to settings").fetchSemanticsNodes().size)
    }

    @Test
    fun openingASectionShowsItsTitleAndABackButtonWithoutASecondMenuButton() {
        show()

        rule.onNodeWithText("Memories").performClick()
        rule.waitForIdle()

        rule.onNodeWithContentDescription("Back to settings").assertExists()
        assertEquals(1, rule.onAllNodesWithText("Memories").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithContentDescription("Conversations").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Search memories").assertExists()
        assertEquals(0, rule.onAllNodesWithText("Instructions").fetchSemanticsNodes().size)
    }

    @Test
    fun backReturnsToTheListOfSections() {
        show()
        rule.onNodeWithText("Memories").performClick()
        rule.waitForIdle()

        rule.onNodeWithContentDescription("Back to settings").performClick()
        rule.waitForIdle()

        rule.onNodeWithText("Instructions").assertExists()
        assertEquals(1, rule.onAllNodesWithContentDescription("Conversations").fetchSemanticsNodes().size)
    }

    @Test
    fun theDesktopLayoutKeepsItsSidebarAndShowsASectionAtOnce() {
        show(touch = false)

        rule.onNodeWithText("Instructions").assertExists()
        rule.onNodeWithText("Accent").assertExists()
        assertEquals(0, rule.onAllNodesWithContentDescription("Back to settings").fetchSemanticsNodes().size)
    }
}
