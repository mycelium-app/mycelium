package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.ui.ColumnSpec
import mycelium.ui.ContentRow
import mycelium.ui.ContentTable
import mycelium.ui.Label
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ContentTableTest {
    @get:Rule
    val rule = createComposeRule()

    private val available = mutableStateOf(900.dp)

    private fun show() {
        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(available.value)) {
                    ContentTable(listOf(ColumnSpec(flexible = true), ColumnSpec(), ColumnSpec())) {
                        ContentRow(header = true) {
                            Label("Name", Modifier.testTag("name-header"), maxLines = 1)
                            Label("Short", Modifier.testTag("short-header"), maxLines = 1)
                            Label("Long", Modifier.testTag("long-header"), maxLines = 1)
                        }

                        ContentRow {
                            Label("Account one with a long enough name to be the main column of this row", Modifier.testTag("name-cell"), maxLines = 1)
                            Label("willow", Modifier.testTag("short-cell"), maxLines = 1)
                            Label("Subscription token", Modifier.testTag("long-cell"), maxLines = 1)
                        }
                    }
                }
            }
        }
    }

    private fun width(tag: String) = rule.onNodeWithTag(tag).getBoundsInRoot().let { it.right - it.left }

    private fun height(tag: String) = rule.onNodeWithTag(tag).getBoundsInRoot().let { it.bottom - it.top }

    @Test
    fun aColumnWithShortContentIsNarrowerThanOneWithLongContentAndTheLongValueStaysOnOneLine() {
        show()

        assertTrue(width("short-cell") < width("long-cell"))
        assertEquals(height("short-cell"), height("long-cell"))
        assertEquals(rule.onNodeWithTag("long-header").getBoundsInRoot().left, rule.onNodeWithTag("long-cell").getBoundsInRoot().left)
    }

    @Test
    fun theFlexibleColumnShrinksBeforeTheOthersWhenSpaceRunsShort() {
        show()
        val shortWide = width("short-cell")
        val longWide = width("long-cell")
        val nameWide = width("name-cell")

        available.value = 400.dp
        rule.waitForIdle()

        assertEquals(shortWide, width("short-cell"))
        assertEquals(longWide, width("long-cell"))
        assertTrue(width("name-cell") < nameWide)
    }
}
