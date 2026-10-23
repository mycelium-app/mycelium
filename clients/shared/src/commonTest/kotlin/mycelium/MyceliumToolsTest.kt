package mycelium

import kotlinx.serialization.json.JsonObject
import mycelium.api.MyceliumJson
import mycelium.state.MyceliumCall
import mycelium.state.MyceliumTool
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.myceliumCall
import mycelium.state.myceliumTool
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull

private fun call(title: String, input: String?, output: String? = null, status: ToolStatus = ToolStatus.Done) =
    ThreadItem.ToolCall("k", "t1", title, "other", title, status, null, output, input = input?.let { MyceliumJson.parseToJsonElement(it) as JsonObject })

class MyceliumToolsTest {
    @Test
    fun everyAgentsNameForATrimmedToolIsRecognised() {
        for (title in listOf("mcp__mycelium__recall", "mycelium.recall", "mycelium__recall", "Tool: mycelium/recall", "mycelium:recall", "Mycelium recall")) {
            assertEquals(MyceliumTool.Recall, myceliumTool(title), title)
        }
    }

    @Test
    fun otherServersAndToolsAreNotRecognised() {
        assertNull(myceliumTool("mcp__orchestrator__recall"))
        assertNull(myceliumTool("mcp__mycelium__unknown"))
        assertNull(myceliumTool("Read package.json"))
    }

    @Test
    fun recallListsTheMemoriesAndWhatWasLeftOut() {
        val output = "11111111-1111-1111-1111-111111111111: The build needs JAVA_HOME.\n22222222-2222-2222-2222-222222222222: Rerun flaky classes alone.\n3 more matches in other groups (infra) left out."
        val parsed = assertIs<MyceliumCall.Recall>(myceliumCall(call("mcp__mycelium__recall", """{"query":"build"}""", output)))

        assertEquals("build", parsed.query)
        assertEquals(listOf("The build needs JAVA_HOME.", "Rerun flaky classes alone."), parsed.memories)
        assertEquals("3 more matches in other groups (infra) left out.", parsed.omitted)
    }

    @Test
    fun recallWithNoMatchesHasNoMemories() {
        val parsed = assertIs<MyceliumCall.Recall>(myceliumCall(call("mycelium.recall", """{"query":"zebra"}""", "No memories match that query.")))

        assertEquals(emptyList(), parsed.memories)
        assertNull(parsed.omitted)
    }

    @Test
    fun recallWithoutAQueryFallsBackToTheGenericCard() {
        assertNull(myceliumCall(call("mcp__mycelium__recall", """{"limit":3}""")))
    }

    @Test
    fun aCallWhoseInputHasNotArrivedYetStillShowsItsVerb() {
        val parsed = assertIs<MyceliumCall.Recall>(myceliumCall(call("mcp__mycelium__recall", null, status = ToolStatus.Running)))

        assertEquals("", parsed.query)
    }

    @Test
    fun rememberShowsTheTextAndWhoItIsSharedWith() {
        val parsed = assertIs<MyceliumCall.Remember>(myceliumCall(call("mcp__mycelium__remember", """{"text":"Use pnpm."}""", "Remembered as 11111111-1111-1111-1111-111111111111, shared with the mycelium group.")))

        assertEquals("Use pnpm.", parsed.text)
        assertEquals("the mycelium group", parsed.sharedWith)
    }

    @Test
    fun proposeTaskShowsTitleBodyAndProject() {
        val parsed = assertIs<MyceliumCall.ProposeTask>(myceliumCall(call("mycelium.propose_task", """{"title":"Bump deps","body":"Do it.","project":"vm"}""")))

        assertEquals(MyceliumCall.ProposeTask("Bump deps", "Do it.", "vm"), parsed)
    }

    @Test
    fun askQuestionShowsTheQuestionAndItsChoices() {
        val parsed = assertIs<MyceliumCall.AskQuestion>(myceliumCall(call("mcp__mycelium__ask_question", """{"question":"Which?","options":["A","B"]}""")))

        assertEquals(MyceliumCall.AskQuestion("Which?", listOf("A", "B")), parsed)
    }

    @Test
    fun askQuestionKeepsTheIdOfTheTaskTheControlPlaneRecordedForIt() {
        val parsed = assertIs<MyceliumCall.AskQuestion>(myceliumCall(call("mcp__mycelium__ask_question", """{"question":"Which?"}""", "Question 66666666-6666-6666-6666-666666666666 asked. End your turn: the answer arrives here as a new message.")))

        assertEquals("66666666-6666-6666-6666-666666666666", parsed.taskId)
    }

    @Test
    fun proposeScheduleDescribesTheCronFromTheResultInWords() {
        val output = "Schedule 11111111-1111-1111-1111-111111111111 proposed (0 20 * * 1-5, Europe/London). It waits for the person to confirm and will not run until they do."
        val parsed = assertIs<MyceliumCall.ProposeSchedule>(myceliumCall(call("mcp__mycelium__propose_schedule", """{"prompt":"Check CI","preset":"weekdays at 20:00"}""", output)))

        assertEquals("Check CI", parsed.prompt)
        assertEquals("Weekdays at 20:00, Europe/London", parsed.schedule)
    }

    @Test
    fun proposeScheduleBeforeItsResultUsesWhatWasAsked() {
        val parsed = assertIs<MyceliumCall.ProposeSchedule>(myceliumCall(call("mcp__mycelium__propose_schedule", """{"prompt":"Check CI","preset":"daily at 09:00","time_zone":"Europe/London"}""", status = ToolStatus.Running)))

        assertEquals("daily at 09:00, Europe/London", parsed.schedule)
    }

    @Test
    fun walkthroughListsTheStepTitles() {
        val parsed = assertIs<MyceliumCall.Walkthrough>(myceliumCall(call("mcp__mycelium__walkthrough", """{"steps":[{"title":"Parse"},{"title":"Render","key":true}]}""")))

        assertEquals(listOf("Parse", "Render"), parsed.steps)
    }

    @Test
    fun walkthroughWithoutStepsFallsBackToTheGenericCard() {
        assertNull(myceliumCall(call("mcp__mycelium__walkthrough", """{"steps":"none"}""")))
    }

    @Test
    fun findConversationsListsWhatWasFound() {
        val output = "11111111-1111-1111-1111-111111111111: Fix billing export (project billing, node laptop, agent claude, status working, last activity 2026-10-23T10:00:00+00:00)\n22222222-2222-2222-2222-222222222222: Plan the release (project none, node laptop, agent claude, status done, last activity 2026-10-22T10:00:00+00:00)"
        val parsed = assertIs<MyceliumCall.FindConversations>(myceliumCall(call("mcp__mycelium__find_conversations", """{"query":"billing"}""", output)))

        assertEquals("billing", parsed.query)
        assertEquals(2, parsed.conversations.size)
        assertEquals("Fix billing export (project billing, node laptop, agent claude, status working, last activity 2026-10-23T10:00:00+00:00)", parsed.conversations[0])
    }

    @Test
    fun findConversationsWithoutAQueryOrMatchesHasNoneListed() {
        val parsed = assertIs<MyceliumCall.FindConversations>(myceliumCall(call("mycelium.find_conversations", """{}""", "No conversations match that query.")))

        assertNull(parsed.query)
        assertEquals(emptyList(), parsed.conversations)
    }

    @Test
    fun sendToConversationShowsTheTargetTheMessageAndTheReason() {
        val output = "Waiting for the person to approve sending this to Fix billing export (proposal 22222222-2222-2222-2222-222222222222)."
        val parsed = assertIs<MyceliumCall.SendToConversation>(myceliumCall(call("mcp__mycelium__send_to_conversation", """{"conversation":"11111111-1111-1111-1111-111111111111","message":"The schema is ready","reason":"You asked for it"}""", output)))

        assertEquals(MyceliumCall.SendToConversation("Fix billing export", "The schema is ready", "You asked for it", "22222222-2222-2222-2222-222222222222"), parsed)
    }

    @Test
    fun sendToConversationWithoutAnOutputFallsBackToTheTargetItWasGiven() {
        val parsed = assertIs<MyceliumCall.SendToConversation>(myceliumCall(call("mcp__mycelium__send_to_conversation", """{"conversation":"Fix billing export","message":"Hi","reason":"Why"}""", status = ToolStatus.Running)))

        assertEquals("Fix billing export", parsed.target)
        assertEquals(MyceliumTool.SendToConversation, parsed.tool)
    }

    @Test
    fun sendToConversationWithoutAMessageFallsBackToTheGenericCard() {
        assertNull(myceliumCall(call("mcp__mycelium__send_to_conversation", """{"conversation":"Fix billing export","reason":"Why"}""")))
    }
}
