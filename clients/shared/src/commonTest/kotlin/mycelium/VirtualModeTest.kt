package mycelium

import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.virtualModeOf
import kotlin.test.Test
import kotlin.test.assertEquals

class VirtualModeTest {
    private fun tool(key: String, title: String, vararg input: Pair<String, String>) =
        ThreadItem.ToolCall(key, key, title, "other", "", ToolStatus.Done, null, "", input = buildJsonObject { input.forEach { (name, value) -> put(name, JsonPrimitive(value)) } })

    private val asked = ThreadItem.UserMessage("u1", "how is disk on the node?")
    private val read = ThreadItem.ToolCall("t1", "t1", "Bash", "execute", "df -h", ToolStatus.Done, null, "")
    private val note = ThreadItem.AgentMessage("a1", "Checking the disk.")
    private val elsewhere = tool("t2", "mcp__mycelium__post_virtual_message", "text" to "Layout?", "chat" to "Discord routing rules")
    private val answer = tool("t3", "mcp__mycelium__post_virtual_message", "text" to "Disk is at 78%.")
    private val items = listOf(asked, read, note, elsewhere, answer, ThreadItem.TurnEnd("e1", "end_turn"))

    @Test
    fun virtualModeKeepsThePersonsMessagesAndInlineVirtualMessagesAndFoldsTheRestIntoOneLine() {
        val shown = virtualModeOf(items, on = true, expanded = emptySet())

        assertEquals(listOf(asked, ThreadItem.Worked("worked:t1", 2, false), answer, ThreadItem.TurnEnd("e1", "end_turn")), shown)
    }

    @Test
    fun anExpandedWorkLineShowsWhatItHidInPlace() {
        val shown = virtualModeOf(items, on = true, expanded = setOf("worked:t1"))

        assertEquals(listOf(asked, ThreadItem.Worked("worked:t1", 2, true), read, note, elsewhere, answer, ThreadItem.TurnEnd("e1", "end_turn")), shown)
    }

    @Test
    fun withVirtualModeOffNothingChanges() {
        assertEquals(items, virtualModeOf(items, on = false, expanded = emptySet()))
    }

    private val notice = ThreadItem.UserMessage("u2", "Mycelium: that call to op failed. Earlier conversations remembered this about it.", fromMycelium = true)
    private val done = ThreadItem.AgentMessage("a2", "Disk was at 84%, now 78%.")

    @Test
    fun aMessageMyceliumInjectedFoldsIntoTheTurnInVirtualModeAndShowsNormallyWithItOff() {
        val turn = listOf(asked, read, notice, answer, ThreadItem.TurnEnd("e1", "end_turn"))

        assertEquals(listOf(asked, ThreadItem.Worked("worked:t1", 1, false), answer, ThreadItem.TurnEnd("e1", "end_turn")), virtualModeOf(turn, on = true, expanded = emptySet()))
        assertEquals(turn, virtualModeOf(turn, on = false, expanded = emptySet()))
    }

    @Test
    fun aTurnThatSentNoVirtualMessageShowsItsLastTextAsOne() {
        val turn = listOf(asked, note, read, done, ThreadItem.TurnEnd("e1", "end_turn"))

        assertEquals(listOf(asked, ThreadItem.Worked("worked:a1", 1, false), ThreadItem.FinalReply("reply:a2", "Disk was at 84%, now 78%."), ThreadItem.TurnEnd("e1", "end_turn")), virtualModeOf(turn, on = true, expanded = emptySet()))
    }

    @Test
    fun aTurnThatSentAVirtualMessageDoesNotAddItsLastTextAsASecond() {
        val turn = listOf(asked, read, answer, done, ThreadItem.TurnEnd("e1", "end_turn"))

        assertEquals(listOf(asked, ThreadItem.Worked("worked:t1", 1, false), answer, ThreadItem.Worked("worked:a2", 0, false), ThreadItem.TurnEnd("e1", "end_turn")), virtualModeOf(turn, on = true, expanded = emptySet()))
    }

    @Test
    fun inVirtualModeTheQueueListsOnlyWhatThePersonTyped() {
        val typed = mycelium.api.QueuedPrompt("q1", "c1", 1, "and the disk?", "2026-11-28T13:19:00Z")
        val relayed = mycelium.api.QueuedPrompt("q2", "c1", 2, "From Mycelium owner: it finished its turn", "2026-11-28T13:19:05Z", origin = "relay")

        assertEquals(listOf(typed), mycelium.state.queueShown(listOf(typed, relayed), virtualMode = true))
        assertEquals(listOf(typed, relayed), mycelium.state.queueShown(listOf(typed, relayed), virtualMode = false))
    }
}
