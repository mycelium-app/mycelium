package mycelium

import kotlinx.serialization.json.Json
import mycelium.api.Event
import mycelium.state.AgentTerminalActivity
import mycelium.state.SplitDirection
import mycelium.state.TerminalFeed
import mycelium.state.Terminal
import mycelium.state.TerminalChunkLimit
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.AppState
import mycelium.state.reduce
import kotlin.test.assertIs
import mycelium.state.TerminalTabs
import mycelium.state.TerminalWorkspace
import mycelium.state.agentTerminalActivity
import mycelium.state.dividers
import mycelium.state.paneIds
import mycelium.state.rects
import kotlin.io.encoding.Base64
import kotlin.test.Test
import kotlin.test.assertEquals

class TerminalWorkspaceTest {
    private fun call(seq: Long, title: String, terminalId: String) = Event("c1", seq, "acp_update", Json.parseToJsonElement("""{"update":{"sessionUpdate":"tool_call","title":"$title","rawInput":{"terminal_id":"$terminalId"}}}"""))

    private fun feed() = listOf("abc" to 0L, "def" to 3L, "ghi" to 6L).fold(TerminalFeed()) { feed, (text, offset) -> feed.with(Base64.encode(text.encodeToByteArray()), offset) }

    private fun replayed(end: Long) = feed().after(end).joinToString("|") { it.decodeToString() }

    @Test
    fun outputTheScrollbackAlreadyCoversIsDroppedAndTheRestIsKept() {
        assertEquals("ef|ghi", replayed(4))
    }

    @Test
    fun aChunkEntirelyBeforeTheScrollbackEndIsDropped() {
        assertEquals("ghi", replayed(6))
    }

    @Test
    fun nothingIsReplayedWhenTheScrollbackCoversEverything() {
        assertEquals("", replayed(9))
    }

    @Test
    fun everythingIsReplayedWhenTheScrollbackWasEmpty() {
        assertEquals("abc|def|ghi", replayed(0))
    }

    @Test
    fun outputWithNoKnownOffsetIsKept() {
        val unknown = TerminalFeed().with(Base64.encode("xy".encodeToByteArray()))

        assertEquals("xy", unknown.after(100).joinToString("") { it.decodeToString() })
    }

    @Test
    fun outputDeliveredAfterTheSnapshotStillDropsBytesAlreadyReplayed() {
        val terminal = Terminal(40, 4)
        val initial = TerminalFeed().with(Base64.encode("abc".encodeToByteArray()), 0)
        terminal.feed("abcdef".encodeToByteArray())
        initial.after(6).forEach { terminal.feed(it) }

        val updated = initial.with(Base64.encode("defghi".encodeToByteArray()), 3)
        updated.after(6, initial.received).forEach { terminal.feed(it) }

        assertEquals("abcdefghi", terminal.screenText().trimEnd())
        assertEquals(emptyList(), updated.after(6, updated.received))
    }

    @Test
    fun replayOffsetsCountBytesWhenTheSnapshotEndsInsideAUtf8Character() {
        val terminal = Terminal(40, 4)
        terminal.feed(byteArrayOf(0xc2.toByte()))
        val feed = TerminalFeed().with(Base64.encode("£x".encodeToByteArray()), 0)
        feed.after(1).forEach { terminal.feed(it) }

        assertEquals("£x", terminal.screenText().trimEnd())
    }

    @Test
    fun offsetsStillMatchChunksAfterOldOutputIsDiscarded() {
        val feed = (0..TerminalChunkLimit).fold(TerminalFeed()) { feed, offset ->
            feed.with(Base64.encode("x".encodeToByteArray()), offset.toLong())
        }

        assertEquals("x", feed.after(TerminalChunkLimit.toLong(), feed.received - 1).single().decodeToString())
        assertEquals(emptyList(), feed.after(TerminalChunkLimit.toLong() + 1))
    }

    @Test
    fun aStreamedByteOffsetReachesTheTerminalReplayFeed() {
        val message = parseStreamMessage("""{"type":"terminal_output","terminal_id":"t1","data":"YWJj","offset":3}""")
        val state = reduce(AppState(), assertIs<StreamMessage.TerminalOutput>(message))

        assertEquals("bc", state.terminalOutput.getValue("t1").after(4).single().decodeToString())
    }

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
