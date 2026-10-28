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
    fun environmentToolsAreHeadedByTheirActionInTheEnvironment() {
        val click = chatToolName("mcp__mycelium__environment_click")
        val launch = chatToolName("mcp.mycelium.environment_launch")

        assertEquals("Click" to "environment", click?.heading to click?.source)
        assertEquals("Open app" to "environment", launch?.heading to launch?.source)
    }

    @Test
    fun codexMcpTitlesNameTheirServerAndHumanHeading() {
        assertEquals(ChatToolName("mycelium", "get_instructions"), chatToolName("mcp.mycelium.get_instructions"))
        assertEquals(ChatToolName("lab", "get_weather"), chatToolName("mcp.lab.get_weather"))
        assertNull(chatToolName("echo mcp.lab.get_weather"))
    }

    @Test
    fun codexMcpArgumentsAndOpaqueResultsAreReadFromTheirAdapterEnvelope() {
        val thread = buildThread(
            listOf(
                update(1, """{"sessionUpdate":"tool_call","toolCallId":"mcp_1","title":"mcp.mycelium.recall","kind":"execute","status":"in_progress","rawInput":{"server":"mycelium","tool":"recall","arguments":{"query":"release"}},"_meta":{"is_mcp_tool_call":true}}"""),
                update(2, """{"sessionUpdate":"tool_call_update","toolCallId":"mcp_1","status":"completed","rawOutput":{"result":{"content":[{"type":"text","text":"No memories match that query."}]},"error":null}}"""),
            ),
        )
        val call = assertIs<ThreadItem.ToolCall>(thread.single())

        assertEquals(listOf("query" to "release"), readableArguments(call.input))
        assertEquals("No memories match that query.", call.output)
        assertEquals(ToolStatus.Done, call.status)
    }

    @Test
    fun codexMcpErrorsAndStructuredResultsRemainReadable() {
        val failed = assertIs<ThreadItem.ToolCall>(buildThread(listOf(
            update(1, """{"sessionUpdate":"tool_call","toolCallId":"mcp_1","title":"mcp.lab.get_weather","kind":"execute","status":"failed","rawInput":{"server":"lab","tool":"get_weather","arguments":{}},"rawOutput":{"result":null,"error":{"message":"Weather service is unavailable."}}}"""),
        )).single())
        val structured = assertIs<ThreadItem.ToolCall>(buildThread(listOf(
            update(1, """{"sessionUpdate":"tool_call","toolCallId":"mcp_2","title":"mcp.lab.get_weather","kind":"execute","status":"completed","rawOutput":{"result":{"content":[],"structuredContent":{"city":"Paris","temperature":18}},"error":null}}"""),
        )).single())

        assertEquals("Weather service is unavailable.", failed.output)
        assertEquals("""{"city":"Paris","temperature":18}""", structured.output)
    }

    @Test
    fun codexBatchedToolExecutionKeepsItsNameCodeAndOutput() {
        val code = "const results = await Promise.all([tools.mcp__mycelium__recall({query: 'release'}), tools.mcp__mycelium__get_instructions({})]); results.forEach(text);"
        val thread = buildThread(listOf(
            update(1, """{"sessionUpdate":"tool_call","toolCallId":"batch","name":"functions.exec","title":"exec","kind":"execute","status":"in_progress","rawInput":{"arguments":{"code":"$code"}}}"""),
            update(2, """{"sessionUpdate":"tool_call_update","toolCallId":"batch","name":"functions.exec","status":"completed","content":[{"type":"content","content":{"type":"text","text":"No memories match that query.\nGlobal instructions"}}]}"""),
        ))
        val call = assertIs<ThreadItem.ToolCall>(thread.single())

        assertEquals("functions.exec", call.title)
        assertEquals(listOf("code" to code), readableArguments(call.input))
        assertEquals("No memories match that query.\nGlobal instructions", call.output)
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

    @Test
    fun aToolCallAwaitsApprovalFromTheRequestUntilItIsAnswered() {
        val call = update(1, """{"sessionUpdate":"tool_call","toolCallId":"call_1","title":"mcp__lab__get_weather","kind":"other","status":"in_progress"}""")
        val asked = event(2, EventKinds.PermissionRequest, """{"request_id":"r1","toolCall":{"toolCallId":"call_1","title":"mcp__lab__get_weather"},"options":[]}""")
        val answered = event(3, EventKinds.PermissionResolved, """{"request_id":"r1","option_id":"allow_once"}""")

        assertEquals(true, (buildThread(listOf(call, asked)).first() as ThreadItem.ToolCall).awaitingApproval)
        assertEquals(false, (buildThread(listOf(call, asked, answered)).first() as ThreadItem.ToolCall).awaitingApproval)
    }

    @Test
    fun aToolCallArrivingAfterItsApprovalRequestStillAwaitsApproval() {
        val asked = event(1, EventKinds.PermissionRequest, """{"request_id":"r1","toolCall":{"toolCallId":"call_1","title":"mcp__lab__get_weather"},"options":[]}""")
        val call = update(2, """{"sessionUpdate":"tool_call","toolCallId":"call_1","title":"mcp__lab__get_weather","kind":"other","status":"in_progress"}""")

        assertEquals(true, buildThread(listOf(asked, call)).filterIsInstance<ThreadItem.ToolCall>().single().awaitingApproval)
    }
}
