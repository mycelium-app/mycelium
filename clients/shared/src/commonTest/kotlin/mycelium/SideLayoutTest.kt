package mycelium

import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.ui.Tokens
import mycelium.ui.panelDemand
import mycelium.ui.panelTakesOver
import mycelium.ui.shownPanelWidth
import mycelium.ui.sidebarDocks
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class SideLayoutTest {
    private val sidebar = Tokens.Size.Sidebar
    private val demand = panelDemand(Tokens.Size.SidePanel, open = true, browser = false)

    @Test
    fun theSidebarDocksOnceItsChatItsPanelAndTheRailFit() {
        assertTrue(sidebarDocks(1166.dp, sidebar, demand))
        assertFalse(sidebarDocks(1165.dp, sidebar, demand))
    }

    @Test
    fun theSidebarNeedsLessRoomWhenNoPanelIsOpen() {
        val railOnly = panelDemand(Tokens.Size.SidePanel, open = false, browser = false)

        assertTrue(sidebarDocks(766.dp, sidebar, railOnly))
        assertFalse(sidebarDocks(765.dp, sidebar, railOnly))
    }

    @Test
    fun theBrowserPanelAsksForItsWiderMinimum() {
        assertEquals(Tokens.Size.SidePanelBrowser + Tokens.Size.Rail, panelDemand(Tokens.Size.SidePanel, open = true, browser = true))
    }

    @Test
    fun thePanelTakesOverTheChatBelowSevenHundredAndSixtyFive() {
        assertFalse(panelTakesOver(765.dp))
        assertTrue(panelTakesOver(764.dp))
    }

    @Test
    fun thePanelNarrowsToItsMinimumBeforeTakingOver() {
        assertEquals(Tokens.Size.SidePanel, shownPanelWidth(Tokens.Size.SidePanel, 1000.dp, browser = false))
        assertEquals(Tokens.Size.SidePanelMin, shownPanelWidth(Tokens.Size.SidePanel, 765.dp, browser = false))
        assertEquals(Tokens.Size.SidePanelMin + 20.dp, shownPanelWidth(Tokens.Size.SidePanel, 785.dp, browser = false))
    }

    @Test
    fun theSavedWidthIsUsedWhileTheAreaIsUnmeasured() {
        assertEquals(Tokens.Size.SidePanel, shownPanelWidth(Tokens.Size.SidePanel, Dp.Unspecified, browser = false))
    }
}
