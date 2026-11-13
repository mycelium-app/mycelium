package mycelium

import kotlin.io.encoding.Base64
import kotlin.test.Test
import kotlin.test.assertEquals
import mycelium.api.ActionItem
import mycelium.api.ActionItemRun
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.ActionItemPhase
import mycelium.state.TerminalFeed
import mycelium.state.TerminalWorkspace
import mycelium.state.cleanTerminalText
import mycelium.state.cleanText
import mycelium.state.formatElapsed
import mycelium.state.phase
import mycelium.state.spokenElapsed

private fun itemRunning(status: String) = ActionItem("i1", "Delete the branch", "2026-10-22T10:00:00Z", command = "git push", run = ActionItemRun("t1", status, "2026-10-22T10:00:00Z"))

class ActionItemRunTest {
    @Test
    fun ansiEscapesAreStrippedFromOutput() {
        assertEquals("error: failed", cleanTerminalText("\u001B[31merror:\u001B[0m failed"))
    }

    @Test
    fun windowsLineEndingsBecomeNewlines() {
        assertEquals("one\ntwo", cleanTerminalText("one\r\ntwo"))
    }

    @Test
    fun trailingNewlinesAreDropped() {
        assertEquals("one", cleanTerminalText("one\r\n\r\n"))
    }

    @Test
    fun aCarriageReturnOverwritesTheLine() {
        assertEquals("100%", cleanTerminalText("10%\r50%\r100%"))
    }

    @Test
    fun onlyTheTailOfLongOutputIsKept() {
        val lines = (1..500).joinToString("\n") { "line $it" }
        val cleaned = cleanTerminalText(lines).lines()

        assertEquals("line 500", cleaned.last())
        assertEquals(200, cleaned.size)
    }

    @Test
    fun aFeedDecodesChunksThatSplitAMultiByteCharacter() {
        val bytes = "café".encodeToByteArray()
        val feed = TerminalFeed().with(Base64.encode(bytes.copyOfRange(0, 4))).with(Base64.encode(bytes.copyOfRange(4, bytes.size)))

        assertEquals("café", feed.cleanText())
    }

    @Test
    fun elapsedTimeReadsAsSecondsThenMinutes() {
        assertEquals("4s", formatElapsed(4))
        assertEquals("1m 5s", formatElapsed(65))
        assertEquals("1h 2m", formatElapsed(3720))
    }

    @Test
    fun elapsedTimeIsSpokenInFullWords() {
        assertEquals("1 second", spokenElapsed(1))
        assertEquals("4 seconds", spokenElapsed(4))
        assertEquals("2 minutes 5 seconds", spokenElapsed(125))
    }

    @Test
    fun aRunningItemSucceedsPromptlyOnExitCodeZero() {
        assertEquals(ActionItemPhase.Running, itemRunning("running").phase(emptyMap()))
        assertEquals(ActionItemPhase.Succeeded, itemRunning("running").phase(mapOf("t1" to 0)))
        assertEquals(ActionItemPhase.Running, itemRunning("running").phase(mapOf("t1" to 1)))
    }

    @Test
    fun anItemWithoutARunWaits() {
        assertEquals(ActionItemPhase.Waiting, ActionItem("i1", "Do it", "2026-10-22T10:00:00Z", command = "ls").phase(emptyMap()))
    }

    @Test
    fun aTerminalExitCarriesItsCodeWhenTheServerSendsOne() {
        assertEquals(StreamMessage.TerminalExited("t1", 2), parseStreamMessage("""{"type":"terminal_exited","terminal_id":"t1","exit_code":2}"""))
    }

    @Test
    fun focusingARunTerminalOpensItsOwnTabWhenTheWorkspaceLacksIt() {
        val workspace = TerminalWorkspace().focusing("t1")

        assertEquals(mapOf(workspace.tabs.current!!.focused to "t1"), workspace.panes)
    }

    @Test
    fun focusingAKnownTerminalSelectsItsTab() {
        val first = TerminalWorkspace().focusing("t1")
        val second = first.focusing("t2")
        val back = second.focusing("t1")

        assertEquals(first.tabs.selected, back.tabs.selected)
        assertEquals(2, back.tabs.tabs.size)
    }
}
