package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.withKeyDown
import androidx.compose.ui.unit.dp
import mycelium.api.GitStatus
import mycelium.ui.CommitArea
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class GitCommitAreaTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun theRemoteActionsSitAtTheRightEdgeOfTheBranchRow() {
        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(330.dp)) {
                    CommitArea(GitStatus("main", upstream = "origin/main", ahead = 2, behind = 0, files = emptyList()), null, "", {}, {}, {}, {})
                }
            }
        }

        val actions = rule.onNodeWithContentDescription("More remote actions").getBoundsInRoot()
        val panel = rule.onRoot().getBoundsInRoot()

        assertTrue(panel.right - actions.right < 24.dp, "The remote actions end ${panel.right - actions.right} from the right edge")
    }

    @Test
    fun theRemoteButtonOffersWhatTheBranchNeedsNext() {
        var status by mutableStateOf(GitStatus("main", upstream = "origin/main", ahead = 0, behind = 0, files = emptyList()))
        rule.setContent { MyceliumTheme(true) { CommitArea(status, null, "", {}, {}, {}, {}) } }

        fun shown(label: String) = rule.onNodeWithText(label, useUnmergedTree = true).assertExists()

        shown("Fetch")
        status = status.copy(ahead = 1)
        shown("Push")
        status = status.copy(behind = 3)
        shown("Pull")
        status = status.copy(upstream = null, ahead = 0, behind = 0)
        rule.onNodeWithContentDescription("Publish main").assertExists()
    }

    @OptIn(ExperimentalTestApi::class)
    @Test
    fun ctrlEnterInTheMessageCommits() {
        var commits = 0

        rule.setContent {
            MyceliumTheme(true) {
                CommitArea(GitStatus("main", ahead = 0, behind = 0, files = emptyList()), null, "fix: a thing", {}, {}, { commits++ }, {})
            }
        }

        val field = rule.onNodeWithContentDescription("Commit message")
        field.performClick()
        field.performKeyInput { pressKey(Key.Enter) }

        assertEquals(0, commits)

        field.performKeyInput { withKeyDown(Key.CtrlLeft) { pressKey(Key.Enter) } }

        assertEquals(1, commits)
    }

    @OptIn(ExperimentalTestApi::class)
    @Test
    fun ctrlShiftEnterInTheMessageAmends() {
        var commits = 0
        var amends = 0

        rule.setContent {
            MyceliumTheme(true) {
                CommitArea(GitStatus("main", ahead = 0, behind = 0, files = emptyList()), null, "", {}, {}, { commits++ }, {}, onAmend = { amends++ })
            }
        }

        val field = rule.onNodeWithContentDescription("Commit message")
        field.performClick()
        field.performKeyInput { withKeyDown(Key.CtrlLeft) { withKeyDown(Key.ShiftLeft) { pressKey(Key.Enter) } } }

        assertEquals(0 to 1, commits to amends)
    }
}
