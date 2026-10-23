package mycelium

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.MyceliumJson
import mycelium.state.ChatToolName
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.buildThread
import mycelium.state.chatToolName
import mycelium.state.readableArguments
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull

private fun event(seq: Long, kind: String, payload: String) = Event("c1", seq, kind, MyceliumJson.parseToJsonElement(payload))

private fun update(seq: Long, body: String) = event(seq, EventKinds.AcpUpdate, """{"update":$body}""")

class ChatToolsTest {
    @Test
    fun aToolTitleNamesItsServerAndAHumanHeading() {
        val name = chatToolName("mcp__lab__get_weather")

        assertEquals(ChatToolName("lab", "get_weather"), name)
        assertEquals("Get weather", name?.heading)
        assertNull(chatToolName("tool_search"))
    }

    @Test
    fun argumentsReadAsPlainTextAndLongValuesAreShortened() {
        val input: JsonObject = buildJsonObject {
            put("city", "Paris")
            put("days", 3)
            put("notes", "x".repeat(300))
        }
        val lines = readableArguments(input)

        assertEquals("city" to "Paris", lines[0])
        assertEquals("days" to "3", lines[1])
        assertEquals(241, lines[2].second.length)
        assertEquals(emptyList(), readableArguments(null))
    }

    @Test
    fun aChatPermissionRequestCarriesItsServerAndArgumentsUntilItIsAnswered() {
        val asked = event(2, EventKinds.PermissionRequest, """{"request_id":"call_1","toolCall":{"toolCallId":"call_1","title":"mcp__lab__get_weather","server":"Lab","rawInput":{"city":"Paris"}},"options":[{"optionId":"allow_once","name":"Allow once","kind":"allow_once"},{"optionId":"reject_once","name":"Deny","kind":"reject_once"}]}""")
        val permission = buildThread(listOf(asked)).single() as ThreadItem.Permission
        val answered = buildThread(listOf(asked, event(3, EventKinds.PermissionResolved, """{"request_id":"call_1","option_id":"allow_once"}"""))).single() as ThreadItem.Permission

        assertEquals(listOf("Allow once", "Deny"), permission.options.map { it.name })
        assertEquals("Lab", permission.server)
        assertEquals(listOf("city" to "Paris"), readableArguments(permission.input))
        assertEquals(false, permission.resolved)
        assertEquals(true, answered.resolved)
    }

    @Test
    fun aChatToolCallAndItsResultFoldIntoOneCardAndANoteStaysQuiet() {
        val thread = buildThread(
            listOf(
                update(1, """{"sessionUpdate":"note","content":{"type":"text","text":"This model does not take tools, so it answered without them."}}"""),
                update(2, """{"sessionUpdate":"tool_call","toolCallId":"call_1","title":"mcp__lab__get_weather","kind":"other","status":"pending","rawInput":{"city":"Paris"}}"""),
                update(3, """{"sessionUpdate":"tool_call_update","toolCallId":"call_1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"Sunny in Paris"}}]}"""),
            ),
        )

        assertIs<ThreadItem.Note>(thread[0])
        val call = thread[1] as ThreadItem.ToolCall
        assertEquals(ToolStatus.Done, call.status)
        assertEquals("Sunny in Paris", call.output)
        assertEquals(listOf("city" to "Paris"), readableArguments(call.input))
    }
}
