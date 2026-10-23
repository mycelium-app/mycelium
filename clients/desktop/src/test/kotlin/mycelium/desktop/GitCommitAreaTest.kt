package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import mycelium.api.GitStatus
import mycelium.ui.CommitArea
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

class GitCommitAreaTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun fetchSitsAtTheRightEdgeOfTheBranchRow() {
        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(330.dp)) {
                    CommitArea(GitStatus("main", ahead = 2, behind = 0, files = emptyList()), null, "", {}, {}, {}, {})
                }
            }
        }

        val fetch = rule.onNodeWithText("Fetch", useUnmergedTree = true).getBoundsInRoot()
        val panel = rule.onRoot().getBoundsInRoot()

        assertTrue(panel.right - fetch.right < 24.dp, "Fetch ends ${panel.right - fetch.right} from the right edge")
    }
}
