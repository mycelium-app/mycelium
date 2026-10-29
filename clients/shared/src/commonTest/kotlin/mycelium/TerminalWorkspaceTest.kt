package mycelium

import kotlinx.serialization.json.Json
import mycelium.api.Event
import mycelium.state.AgentTerminalActivity
import mycelium.state.SplitDirection
import mycelium.state.TerminalTabs
import mycelium.state.TerminalWorkspace
import mycelium.state.agentTerminalActivity
import mycelium.state.dividers
import mycelium.state.paneIds
import mycelium.state.rects
import kotlin.test.Test
import kotlin.test.assertEquals

class TerminalWorkspaceTest {
    private fun call(seq: Long, title: String, terminalId: String) = Event("c1", seq, "acp_update", Json.parseToJsonElement("""{"update":{"sessionUpdate":"tool_call","title":"$title","rawInput":{"terminal_id":"$terminalId"}}}"""))

    @Test
    fun theAgentIsShownReadingOrTypingInTheTerminalItLastUsedWhileItWorks() {
        val events = listOf(call(1, "mcp__mycelium__terminal_read", "t1"), call(2, "mcp__mycelium__terminal_type", "t1"), call(3, "mcp__mycelium__terminal_read", "t2"))

        assertEquals(mapOf("t1" to AgentTerminalActivity.Typing, "t2" to AgentTerminalActivity.Reading), agentTerminalActivity(events, working = true))
    }

    @Test
    fun nothingIsShownOnceTheAgentIsNoLongerWorking() {
        assertEquals(emptyMap(), agentTerminalActivity(listOf(call(1, "mcp__mycelium__terminal_type", "t1")), working = false))
    }

    @Test
    fun otherToolCallsAreNotTerminalActivity() {
        assertEquals(emptyMap(), agentTerminalActivity(listOf(call(1, "mcp__mycelium__remember", "t1")), working = true))
    }

    @Test
    fun draggingADividerMovesTheBoundaryBetweenItsPanes() {
        val tabs = TerminalTabs().open().split(SplitDirection.Right)
        val tab = tabs.current!!
        val divider = tab.layout.dividers().single()
        val resized = tabs.resize(tab.id, divider.path, 0.7f).current!!.layout
        val panes = resized.paneIds()

        assertEquals(0.7f, resized.rects().getValue(panes[0]).width)
    }

    @Test
    fun aDividerCannotShrinkAPaneAwayEntirely() {
        val tabs = TerminalTabs().open().split(SplitDirection.Down)
        val tab = tabs.current!!
        val resized = tabs.resize(tab.id, tab.layout.dividers().single().path, 0.01f).current!!.layout

        assertEquals(0.15f, resized.rects().getValue(resized.paneIds()[0]).height)
    }

    @Test
    fun anInnerDividerResizesOnlyItsOwnSplit() {
        val tabs = TerminalTabs().open().split(SplitDirection.Right).let { it.focus(it.current!!.layout.paneIds()[0]) }.split(SplitDirection.Down)
        val tab = tabs.current!!
        val inner = tab.layout.dividers().first { it.path.isNotEmpty() }
        val resized = tabs.resize(tab.id, inner.path, 0.3f).current!!.layout

        assertEquals(0.5f, resized.dividers().first { it.path.isEmpty() }.ratio)
        assertEquals(0.3f, resized.dividers().first { it.path.isNotEmpty() }.ratio)
    }

    @Test
    fun returningToRunningTerminalsOpensOneTabForEachAndRemembersWhichTerminalIsInIt() {
        val workspace = TerminalWorkspace().adopting(listOf("t1", "t2"))

        assertEquals(listOf("Terminal 1", "Terminal 2"), workspace.tabs.tabs.map { it.title })
        assertEquals(listOf("t1", "t2"), workspace.tabs.tabs.map { workspace.panes.getValue(it.focused) })
    }
}
