package mycelium

import mycelium.state.SplitDirection
import mycelium.state.TerminalLayout
import mycelium.state.TerminalTabs
import mycelium.state.paneIds
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class TerminalTabsTest {
    @Test
    fun openingATabSelectsItAndNumbersItsTitle() {
        val tabs = TerminalTabs().open().open()

        assertEquals(listOf("Terminal 1", "Terminal 2"), tabs.tabs.map { it.title })
        assertEquals("Terminal 2", tabs.current?.title)
    }

    @Test
    fun splittingRightPutsANewPaneBesideTheFocusedOneAndFocusesIt() {
        val opened = TerminalTabs().open()
        val first = opened.current!!.focused
        val split = opened.split(SplitDirection.Right)
        val layout = split.current!!.layout as TerminalLayout.Split

        assertEquals(SplitDirection.Right, layout.direction)
        assertEquals(TerminalLayout.Pane(first), layout.first)
        assertEquals(split.current!!.focused, (layout.second as TerminalLayout.Pane).id)
    }

    @Test
    fun splittingDownInsideASplitOnlyDividesTheFocusedPane() {
        val split = TerminalTabs().open().split(SplitDirection.Right)
        val left = split.current!!.layout.paneIds().first()
        val down = split.focus(left).split(SplitDirection.Down)
        val layout = down.current!!.layout as TerminalLayout.Split

        assertEquals(SplitDirection.Right, layout.direction)
        assertEquals(SplitDirection.Down, (layout.first as TerminalLayout.Split).direction)
        assertEquals(3, layout.paneIds().size)
    }

    @Test
    fun splittingWithNoTerminalOpensTheFirstOne() {
        assertEquals(1, TerminalTabs().split(SplitDirection.Down).tabs.size)
    }

    @Test
    fun closingOnePaneOfASplitLeavesTheOtherAndMovesFocusToIt() {
        val split = TerminalTabs().open().split(SplitDirection.Right)
        val ids = split.current!!.layout.paneIds()
        val closed = split.closePane(ids[1])

        assertEquals(TerminalLayout.Pane(ids[0]), closed.current!!.layout)
        assertEquals(ids[0], closed.current!!.focused)
    }

    @Test
    fun closingTheLastPaneRemovesTheTabAndSelectsANeighbour() {
        val tabs = TerminalTabs().open().open()
        val closed = tabs.closeFocused()

        assertEquals(listOf("Terminal 1"), closed.tabs.map { it.title })
        assertEquals("Terminal 1", closed.current?.title)
    }

    @Test
    fun closingEveryTabLeavesNothingSelected() {
        val closed = TerminalTabs().open().closeFocused()

        assertEquals(emptyList(), closed.tabs)
        assertNull(closed.current)
    }

    @Test
    fun closingATabClosesAllItsPanes() {
        val tabs = TerminalTabs().open().split(SplitDirection.Right).split(SplitDirection.Down)
        val closed = tabs.closeTab(tabs.current!!.id)

        assertEquals(emptyList(), closed.tabs)
    }

    @Test
    fun tabNumbersAreNotReusedAfterAClose() {
        val tabs = TerminalTabs().open().open().closeFocused().open()

        assertEquals(listOf("Terminal 1", "Terminal 3"), tabs.tabs.map { it.title })
    }
}
