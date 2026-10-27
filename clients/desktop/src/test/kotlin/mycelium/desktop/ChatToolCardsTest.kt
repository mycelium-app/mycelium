package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import kotlinx.serialization.json.Json
import mycelium.api.Event
import mycelium.state.ThreadItem
import mycelium.state.buildThread
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun update(seq: Long, body: String) = Event("c1", seq, "acp_update", Json.parseToJsonElement("""{"update":$body}"""))

private fun event(seq: Long, kind: String, payload: String) = Event("c1", seq, kind, Json.parseToJsonElement(payload))

private const val Request = """{"request_id":"call_2","toolCall":{"toolCallId":"call_2","title":"mcp__lab__get_weather","server":"lab","rawInput":{"city":"Paris","days":3}},"options":[{"optionId":"allow_once","name":"Allow once","kind":"allow_once"},{"optionId":"allow_always","name":"Always allow this tool","kind":"allow_always"},{"optionId":"reject_once","name":"Deny","kind":"reject_once"}]}"""

internal val ChatToolThread = listOf(
    event(1, "user_message", """{"text":"What is the weather in Paris?"}"""),
    update(2, """{"sessionUpdate":"tool_call","toolCallId":"call_1","title":"tool_search","kind":"other","status":"pending","rawInput":{"query":"weather forecast"}}"""),
    update(3, """{"sessionUpdate":"tool_call_update","toolCallId":"call_1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"These tools are now loaded."}}],"rawOutput":{"loaded":["mcp__lab__get_weather"]}}"""),
    update(4, """{"sessionUpdate":"tool_call","toolCallId":"call_0","title":"mcp__lab__list_files","kind":"other","status":"pending","rawInput":{"path":"/srv"}}"""),
    update(5, """{"sessionUpdate":"tool_call_update","toolCallId":"call_0","status":"completed","content":[{"type":"content","content":{"type":"text","text":"notes.txt\nreport.pdf"}}]}"""),
    update(6, """{"sessionUpdate":"tool_call","toolCallId":"call_2","title":"mcp__lab__get_weather","kind":"other","status":"pending","rawInput":{"city":"Paris","days":3}}"""),
    event(7, "permission_request", Request),
)

class ChatToolCardsTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show(events: List<Event>, answered: MutableList<Pair<String, String>> = mutableListOf(), agent: String = "chat") {
        val items = buildThread(events)
        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) {
                    ThreadView(items, onAnswer = { request, option -> answered += request to option }, onStopWorkflow = {}, agent = agent)
                }
            }
        }
    }

    @Test
    fun codexMcpCallsShowHumanNamesArgumentsAndResultsBesideGuardianReviews() {
        show(listOf(
            update(1, """{"sessionUpdate":"tool_call","toolCallId":"guardian_assessment:1","title":"Guardian Review","kind":"think","status":"completed","content":[{"type":"content","content":{"type":"text","text":"Status: Approved"}}]}"""),
            update(2, """{"sessionUpdate":"tool_call","toolCallId":"recall","title":"mcp.mycelium.recall","kind":"execute","status":"completed","rawInput":{"server":"mycelium","tool":"recall","arguments":{"query":"release"}},"rawOutput":{"result":{"content":[{"type":"text","text":"No memories match that query."}]},"error":null}}"""),
            update(3, """{"sessionUpdate":"tool_call","toolCallId":"instructions","title":"mcp.mycelium.get_instructions","kind":"execute","status":"completed","rawInput":{"server":"mycelium","tool":"get_instructions","arguments":{"layer":"global"}},"rawOutput":{"result":{"content":[{"type":"text","text":"Use British English."}]},"error":null}}"""),
        ), agent = "codex")

        rule.onNodeWithContentDescription("Guardian Review").assertExists()
        rule.onNodeWithContentDescription("Searched memory for “release”").assertExists()
        rule.onNodeWithContentDescription("Get instructions, mycelium, done").assertExists()
        rule.onNodeWithContentDescription("layer: global").assertExists()
        rule.onNodeWithText("Use British English.", useUnmergedTree = true).assertExists()
    }

    @Test
    fun codexExternalMcpAndBatchedFunctionsExecUseToolCards() {
        show(listOf(
            update(1, """{"sessionUpdate":"tool_call","toolCallId":"weather","title":"mcp.lab.get_weather","kind":"execute","status":"completed","rawInput":{"server":"lab","tool":"get_weather","arguments":{"city":"Paris"}},"rawOutput":{"result":{"content":[{"type":"text","text":"Sunny in Paris"}]},"error":null}}"""),
            update(2, """{"sessionUpdate":"tool_call","toolCallId":"batch","name":"functions.exec","title":"exec","kind":"execute","status":"completed","rawInput":{"arguments":"const results = await Promise.all([tools.mcp__mycelium__recall({query: 'release'}), tools.mcp__mycelium__get_instructions({})]); results.forEach(text);"},"content":[{"type":"content","content":{"type":"text","text":"No memories match that query.\nUse British English."}}]}"""),
        ), agent = "codex")

        rule.onNodeWithContentDescription("Get weather, lab, done").assertExists()
        rule.onNodeWithContentDescription("city: Paris").assertExists()
        rule.onNodeWithText("Sunny in Paris", useUnmergedTree = true).assertExists()
        rule.onNodeWithContentDescription("Run tools, functions, done").assertExists()
        rule.onNodeWithText("No memories match that query.\nUse British English.", useUnmergedTree = true).assertExists()
    }

    @Test
    fun aPendingApprovalNamesTheToolTheServerAndTheArgumentsAndOffersApproveDenyAndAlwaysApprove() {
        show(ChatToolThread)

        rule.onNodeWithText("Server: lab").assertExists()
        rule.onNodeWithContentDescription("city: Paris").assertExists()
        rule.onNodeWithContentDescription("days: 3").assertExists()
        rule.onNodeWithText("Approve").assertExists()
        rule.onNodeWithText("Deny").assertExists()
        rule.onNodeWithContentDescription("More ways to approve").performClick()
        rule.onNodeWithText("Always approve").assertExists()
    }

    @Test
    fun choosingAnOptionAnswersTheRequestByItsIdAndAnAnsweredCardHasNoChoices() {
        val answered = mutableListOf<Pair<String, String>>()
        show(ChatToolThread, answered)

        rule.onNodeWithContentDescription("More ways to approve").performClick()
        rule.onNodeWithText("Always approve").performClick()

        assertEquals(listOf("call_2" to "allow_always"), answered)

        show(ChatToolThread + event(8, "permission_resolved", """{"request_id":"call_2","option_id":"allow_always"}"""))
        assertEquals(1, rule.onAllNodesWithText("Answered").fetchSemanticsNodes().size)
    }

    @Test
    fun finishedToolCallsShowTheirArgumentsAndResultAsCards() {
        show(ChatToolThread)

        rule.onNodeWithContentDescription("Searched for tools, done").assertExists()
        rule.onNodeWithContentDescription("query: weather forecast").assertExists()
        rule.onNodeWithContentDescription("List files, lab, done").assertExists()
        rule.onNodeWithContentDescription("path: /srv").assertExists()
    }

    @Test
    fun aFailedCallShowsWhyAndANoteStaysAsQuietText() {
        show(
            listOf(
                update(1, """{"sessionUpdate":"note","content":{"type":"text","text":"This model does not take tools, so it answered without them."}}"""),
                update(2, """{"sessionUpdate":"tool_call","toolCallId":"c","title":"mcp__lab__get_weather","kind":"other","status":"pending","rawInput":{"city":"Paris"}}"""),
                update(3, """{"sessionUpdate":"tool_call_update","toolCallId":"c","status":"failed","content":[{"type":"content","content":{"type":"text","text":"The person declined this tool call."}}]}"""),
            ),
        )

        rule.onNodeWithContentDescription("Get weather, lab, failed").assertExists()
        rule.onNodeWithText("The person declined this tool call.").assertExists()
        rule.onNodeWithText("This model does not take tools, so it answered without them.").assertExists()
    }
}
