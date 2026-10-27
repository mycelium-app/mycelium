package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.requiredWidth
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.unit.DpRect
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.ui.Col
import mycelium.ui.MyceliumTheme
import mycelium.ui.Overflow
import mycelium.ui.Table
import mycelium.ui.TableCell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class TableTest {
    @get:Rule
    val rule = createComposeRule()

    private val widths = listOf(320.dp, 480.dp, 900.dp, 1600.dp)
    private val available = mutableStateOf(900.dp)
    private val longTitle = "A title that runs on and on and on well past any column in the table"
    private val longPath = "/home/claude/projects/mycelium/clients/shared/src/commonMain/kotlin/mycelium/ui"
    private val longNote = "Notes that are long enough to wrap onto a second line in a narrow wrapping column"

    private fun columns(wrap: Boolean = false, weight: Float = 1f) = listOf(
        Col("Name", 120.dp, weight = weight),
        Col("Path", 120.dp, max = 300.dp),
        Col("Count", 56.dp, numeric = true),
        Col("Status", 72.dp),
        Col("Notes", 100.dp, overflow = if (wrap) Overflow.Wrap else Overflow.Truncate, priority = 1),
    )

    private val data = listOf(
        listOf(longTitle, longPath, "5", "", longNote),
        listOf("Short", "/tmp", "12345", "Ready", "ok"),
        listOf("Third", "/var", "7", "Idle", "fine"),
    )

    private fun show(
        columns: List<Col> = columns(),
        rows: Int = data.size,
        fill: Boolean = true,
        lazy: Boolean = false,
        cell: (Int, Int) -> String = { row, column -> data[row][column] },
        onRow: ((Int) -> Unit)? = { },
    ) {
        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.requiredWidth(available.value).height(400.dp).testTag("host")) {
                    Table(columns, rows, { row, column -> TableCell.Text(cell(row, column)) }, Modifier.testTag("table"), fill = fill, lazy = lazy, onRow = onRow, rowDescription = { "row $it" })
                }
            }
        }
    }

    private fun atEveryWidth(check: (Dp) -> Unit) {
        for (width in widths) {
            available.value = width
            rule.waitForIdle()
            check(width)
        }
    }

    private fun boundsOf(interaction: SemanticsNodeInteraction): DpRect {
        val node = interaction.fetchSemanticsNode()
        val position = node.positionInRoot

        return with(rule.density) { DpRect(position.x.toDp(), position.y.toDp(), (position.x + node.size.width).toDp(), (position.y + node.size.height).toDp()) }
    }

    private fun bounds(text: String) = boundsOf(rule.onNodeWithText(text, useUnmergedTree = true))

    private fun height(text: String) = bounds(text).let { it.bottom - it.top }

    @Test
    fun truncateCellsAreOneLineAtEveryWidth() {
        show()

        atEveryWidth { width ->
            assertEquals(height("Short"), height(longTitle), "name at $width")
            assertEquals(height("Short"), height(longPath), "path at $width")
        }
    }

    @Test
    fun noColumnNarrowerThanMin() {
        show()

        atEveryWidth { width ->
            assertTrue(bounds("Path").left - bounds("Name").left >= 120.dp, "Name at $width")
            assertTrue(bounds("Status").left - bounds("Path").left >= 176.dp, "Path and Count at $width")
        }
    }

    @Test
    fun rightAlignedHeaderKeepsItsEndPaddingAtEveryWidth() {
        show()

        atEveryWidth { width ->
            assertTrue(bounds("Status").left - bounds("Count").right >= 24.dp, "Count header and cells keep 12dp end padding at $width")
            assertTrue(bounds("Status").left - bounds("12345").right >= 24.dp, "Count cells keep 12dp end padding at $width")
        }
    }

    @Test
    fun headerScrollsWithBody() {
        available.value = 320.dp
        show()

        val before = bounds("Name").left
        assertEquals(before, bounds("Short").left)

        rule.onNode(hasScrollAction()).performSemanticsAction(SemanticsActions.ScrollBy) { it(100f, 0f) }
        rule.waitForIdle()

        assertTrue(bounds("Name").left < before)
        assertEquals(bounds("Name").left, bounds("Short").left)
        assertEquals(bounds("Status").left, bounds("Ready").left)
    }

    @Test
    fun headerAndBodyEdgesMatchForFirstAndLast200LazyRows() {
        available.value = 900.dp
        show(rows = 200, lazy = true, cell = { row, column -> if (column == 0) "item $row" else "cell $row $column" })

        assertEquals(bounds("Name").left, bounds("item 0").left)
        assertEquals(bounds("Path").left, bounds("cell 0 1").left)

        rule.onNode(hasScrollToIndexAction()).performScrollToIndex(199)
        rule.waitForIdle()

        assertEquals(bounds("Name").left, bounds("item 199").left)
        assertEquals(bounds("Status").left, bounds("cell 199 3").left)
    }

    @Test
    fun rowLinesSpanBorderInnerEdges() {
        show()

        atEveryWidth { width ->
            val table = boundsOf(rule.onNodeWithTag("table"))
            val row = boundsOf(rule.onNodeWithContentDescription("row 1"))

            if (width >= 900.dp) {
                assertEquals(table.left + 1.dp, row.left, "left at $width")
                assertEquals(table.right - 1.dp, row.right, "right at $width")
            }

            assertEquals(width, table.right - table.left, "table fills $width")
        }
    }

    @Test
    fun tableHugsContentWhenNoWeightAndNotFill() {
        available.value = 1600.dp
        show(columns = columns(weight = 0f), fill = false)

        val table = boundsOf(rule.onNodeWithTag("table"))

        assertTrue(table.right - table.left < 1600.dp)
        assertTrue(bounds("Notes").right < table.right)
    }

    @Test
    fun numericRightEdgesAlign() {
        show()

        atEveryWidth { width ->
            assertEquals(bounds("5").right, bounds("12345").right, "at $width")
            assertEquals(bounds("7").right, bounds("12345").right, "at $width")
        }
    }

    @Test
    fun wrapColumnGrowsRowNeighboursTopAligned() {
        available.value = 900.dp
        show(columns = columns(wrap = true).map { if (it.title == "Notes") it.copy(max = 120.dp) else it })

        val wrapped = bounds(longNote)
        val neighbour = bounds("Short")

        assertTrue(wrapped.bottom - wrapped.top > neighbour.bottom - neighbour.top)
        assertEquals(bounds(longTitle).top, wrapped.top)
        assertEquals(bounds("Path").left, bounds(longPath).left)
    }

    @Test
    fun lowPriorityColumnHiddenBelowBreakpoint() {
        show()

        available.value = 320.dp
        rule.waitForIdle()
        rule.onNodeWithText("Notes").assertDoesNotExist()

        available.value = 900.dp
        rule.waitForIdle()
        rule.onNodeWithText("Notes").assertExists()
    }
}
