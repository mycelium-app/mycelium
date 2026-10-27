package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTag
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.moveTo
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performMouseInput
import androidx.compose.ui.unit.dp
import mycelium.ui.Label
import mycelium.ui.MyceliumTheme
import mycelium.ui.ScrollbarShown
import mycelium.ui.scrollbar
import mycelium.ui.verticalScrollWithBar
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private val Shown = SemanticsMatcher.keyIsDefined(ScrollbarShown)

@Composable
private fun Rows(count: Int) {
    Column(Modifier.size(300.dp, 200.dp).semantics { testTag = "area" }.verticalScrollWithBar()) {
        repeat(count) { Label("Row $it", Modifier.height(30.dp)) }
    }
}

class ScrollbarTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun aLongListShowsAScrollbarWhileThePointerIsOverIt() {
        rule.setContent { MyceliumTheme(true) { Rows(40) } }

        rule.onAllNodes(Shown).assertCountEquals(0)

        rule.onNodeWithTag("area").performMouseInput { moveTo(center) }
        rule.waitForIdle()

        rule.onAllNodes(Shown).assertCountEquals(1)
    }

    @Test
    fun theScrollbarGoesWhenThePointerLeaves() {
        rule.setContent { MyceliumTheme(true) { Rows(40) } }

        rule.onNodeWithTag("area").performMouseInput { moveTo(center); exit() }
        rule.waitForIdle()

        rule.onAllNodes(Shown).assertCountEquals(0)
    }

    @Test
    fun aShortListNeverShowsAScrollbar() {
        rule.setContent { MyceliumTheme(true) { Rows(3) } }

        rule.onNodeWithTag("area").performMouseInput { moveTo(center) }
        rule.waitForIdle()

        rule.onAllNodes(Shown).assertCountEquals(0)
    }

    @Test
    fun aLazyListShowsAScrollbarOnHover() {
        rule.setContent {
            MyceliumTheme(true) {
                val state = rememberLazyListState()

                LazyColumn(Modifier.size(300.dp, 200.dp).scrollbar(state).semantics { testTag = "area" }, state) {
                    items((0 until 100).toList()) { Label("Row $it", Modifier.height(30.dp)) }
                }
            }
        }

        rule.onNodeWithTag("area").performMouseInput { moveTo(center) }
        rule.waitForIdle()

        rule.onAllNodes(Shown).assertCountEquals(1)
    }

    @Test
    fun draggingTheThumbScrollsTheList() {
        val state = ScrollHolder()

        rule.setContent {
            MyceliumTheme(true) {
                val scroll = rememberScrollState()
                state.scroll = scroll

                Box(Modifier.size(300.dp, 200.dp).semantics { testTag = "area" }.verticalScrollWithBar(scroll)) {
                    Column { repeat(40) { Label("Row $it", Modifier.height(30.dp)) } }
                }
            }
        }

        rule.onNodeWithTag("area").performMouseInput { moveTo(center) }
        rule.waitForIdle()
        rule.onNodeWithTag("area").performMouseInput {
            val thumb = Offset(width - 6f, 10f)

            moveTo(thumb)
            press()
            moveTo(thumb + Offset(0f, 60f))
            release()
        }
        rule.waitForIdle()

        assertTrue(state.scroll.value > 0)
    }
}

private class ScrollHolder {
    lateinit var scroll: androidx.compose.foundation.ScrollState
}
