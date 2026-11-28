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
}
