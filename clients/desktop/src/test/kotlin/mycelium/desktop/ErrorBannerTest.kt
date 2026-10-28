package mycelium.desktop

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import mycelium.ui.ButtonSize
import mycelium.ui.ButtonVariant
import mycelium.ui.ErrorBanner
import mycelium.ui.MyButton
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ErrorBannerTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun anActionSitsInsideTheAlertAndRunsWhenSelected() {
        var retries = 0

        rule.setContent {
            MyceliumTheme(true) {
                ErrorBanner("Could not load the folders.", onDismiss = {}, actions = { MyButton({ retries += 1 }, text = "Retry", description = "Retry", size = ButtonSize.Sm, variant = ButtonVariant.Outline) })
            }
        }

        val alert = rule.onNodeWithContentDescription("Error: Could not load the folders.").getBoundsInRoot()
        val retry = rule.onNodeWithContentDescription("Retry").getBoundsInRoot()

        assertTrue(retry.left >= alert.left && retry.right <= alert.right && retry.top >= alert.top && retry.bottom <= alert.bottom)

        rule.onNodeWithContentDescription("Retry").performClick()
        assertEquals(1, retries)
    }

    @Test
    fun dismissRemovesTheAlert() {
        rule.setContent {
            MyceliumTheme(true) {
                var failure by remember { mutableStateOf<String?>("Could not load the folders.") }

                failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }
            }
        }

        rule.onNodeWithContentDescription("Dismiss").performClick()

        assertEquals(0, rule.onAllNodesWithContentDescription("Error: Could not load the folders.").fetchSemanticsNodes().size)
    }
}
