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
import mycelium.state.buildThread
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun update(seq: Long, body: String) = Event("c1", seq, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":$body}"""))

private fun call(id: String, seq: Long, title: String, input: String, status: String, output: String? = null): List<Event> {
    val text = output?.let { Json.encodeToString(kotlinx.serialization.serializer<String>(), it) }
    val content = text?.let { ""","content":[{"type":"content","content":{"type":"text","text":$it}}]""" }.orEmpty()

    return listOf(
        update(seq, """{"sessionUpdate":"tool_call","toolCallId":"$id","title":"$title","kind":"other","status":"pending","rawInput":{}}"""),
        update(seq + 1, """{"sessionUpdate":"tool_call_update","toolCallId":"$id","status":"$status","rawInput":$input$content}"""),
    )
}

private const val MemoryOne = "11111111-1111-1111-1111-111111111111"

internal val MyceliumThread = call("t1", 1, "mcp__mycelium__recall", """{"query":"build"}""", "completed", "$MemoryOne: The build needs JAVA_HOME set before gradle runs.\n22222222-2222-2222-2222-222222222222: Rerun flaky test classes alone.\n33333333-3333-3333-3333-333333333333: ProjectsTest fails four cases in full runs.\n44444444-4444-4444-4444-444444444444: Disk is tight, do not delete outside build dirs.\n2 more matches in other groups (infra) left out. Search them with groups: [\"name\"], or every group with groups: \"all\".") +
    call("t2", 3, "mycelium.recall", """{"query":"zebra"}""", "completed", "No memories match that query.") +
    call("t3", 5, "mcp__mycelium__remember", """{"text":"Gradle tests need JAVA_HOME from mise."}""", "completed", "Remembered as $MemoryOne, shared with the mycelium group.") +
    call("t4", 7, "mcp__mycelium__propose_task", """{"title":"Bump kotlin to 2.3","body":"Update the catalog and fix the deprecation warnings in the shared module.","project":"mycelium"}""", "completed", "Task 55555555-5555-5555-5555-555555555555 proposed. The person can approve it from their Tasks board.") +
    call("t5", 9, "mcp__mycelium__ask_question", """{"question":"Should the walkthrough include test files?","options":["Yes","No","Only changed tests"]}""", "completed", "Question 66666666-6666-6666-6666-666666666666 asked. End your turn: the answer arrives here as a new message.") +
    call("t6", 11, "mcp__mycelium__propose_schedule", """{"prompt":"Check CI and summarise failures","preset":"weekdays at 20:00"}""", "completed", "Schedule 77777777-7777-7777-7777-777777777777 proposed (0 20 * * 1-5, Europe/London). It waits for the person to confirm and will not run until they do.") +
    call("t7", 13, "mcp__mycelium__walkthrough", """{"steps":[{"title":"Parse the tool name"},{"title":"Render the card"},{"title":"Cover with tests"},{"title":"Wire the thread"}]}""", "completed", "Walkthrough stored with 4 steps. The person can read it in the Walkthrough tab.") +
    call("t8", 15, "mcp__mycelium__recall", """{"query":"deploy"}""", "in_progress") +
    call("t9", 17, "mcp__mycelium__remember", """{"text":"x"}""", "failed", "Nothing was stored. This conversation is not in a group, so remember it without a scope.")

class MyceliumCardsTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show(events: List<Event>) {
        val items = buildThread(events)
        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) } } }
    }

    @Test
    fun aRecallShowsTheSearchAndTheFirstMemoriesWithAControlForTheRest() {
        show(call("t1", 1, "mcp__mycelium__recall", """{"query":"build"}""", "completed", "$MemoryOne: The build needs JAVA_HOME.\n22222222-2222-2222-2222-222222222222: Rerun flaky classes alone.\n33333333-3333-3333-3333-333333333333: Third.\n44444444-4444-4444-4444-444444444444: Fourth."))

        rule.onNodeWithContentDescription("Searched memory for “build”").assertExists()
        rule.onNodeWithText("The build needs JAVA_HOME.").assertExists()
        assertEquals(0, rule.onAllNodesWithText("Fourth.").fetchSemanticsNodes().size)

        rule.onNodeWithText("Show all 4").performClick()

        rule.onNodeWithText("Fourth.").assertExists()
    }

    @Test
    fun aRecallWithNoMatchesSaysSo() {
        show(call("t2", 1, "mycelium.recall", """{"query":"zebra"}""", "completed", "No memories match that query."))

        rule.onNodeWithText("Nothing in memory matches “zebra”.").assertExists()
    }

    @Test
    fun aRememberShowsWhatWasStoredAndWhoSeesIt() {
        show(call("t3", 1, "mcp__mycelium__remember", """{"text":"Use pnpm."}""", "completed", "Remembered as $MemoryOne, shared with the mycelium group."))

        rule.onNodeWithContentDescription("Remembered").assertExists()
        rule.onNodeWithText("Use pnpm.").assertExists()
        rule.onNodeWithText("Shared with the mycelium group").assertExists()
    }

    @Test
    fun aRunningCallShowsTheVerbInsteadOfTheRawName() {
        show(call("t8", 1, "mcp__mycelium__recall", """{"query":"deploy"}""", "in_progress"))

        rule.onNodeWithContentDescription("Searching memory for “deploy”…").assertExists()
        rule.onNodeWithText("Running").assertExists()
        assertEquals(0, rule.onAllNodesWithText("mcp__mycelium__recall").fetchSemanticsNodes().size)
    }

    @Test
    fun aFailedCallShowsItsErrorPlainly() {
        show(call("t9", 1, "mcp__mycelium__remember", """{"text":"x"}""", "failed", "Nothing was stored. This conversation is not in a group."))

        rule.onNodeWithText("Failed").assertExists()
        rule.onNodeWithText("Nothing was stored. This conversation is not in a group.").assertExists()
    }

    @Test
    fun aCallWithUnreadableInputKeepsTheGenericRowWithAReadableLabel() {
        show(call("t1", 1, "mcp__mycelium__recall", """{"limit":3}""", "completed", "ok"))

        rule.onNodeWithText("Search memory").assertExists()
        assertEquals(0, rule.onAllNodesWithText("mcp__mycelium__recall").fetchSemanticsNodes().size)
    }

    @Test
    fun everyCardRendersInTheDarkTheme() {
        val items = buildThread(MyceliumThread)
        val file = renderAt("mycelium-cards", true, false) {
            Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) }
        }

        assertEquals(true, file.length() > 0)
    }
}
