package mycelium

import kotlinx.serialization.json.JsonElement
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.MyceliumJson
import mycelium.state.DiffLine
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.buildThread
import mycelium.state.diffLines
import mycelium.state.suggestedReplies
import mycelium.state.turnIsOpen
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

private fun json(text: String): JsonElement = MyceliumJson.parseToJsonElement(text)

private fun event(seq: Long, kind: String, payload: String) = Event("c1", seq, kind, json(payload))

private fun update(seq: Long, body: String) = event(seq, EventKinds.AcpUpdate, """{"sessionId":"s","update":$body}""")

private fun chunk(seq: Long, text: String) =
    update(seq, """{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"$text"}}""")

class ThreadTest {
    @Test
    fun thoughtChunksJoinIntoOneThoughtThatEndsWhenTheReplyStarts() {
        val thinking = buildThread(listOf(update(1, """{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Checking "}}"""), update(2, """{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"the tests"}}""")))
        val answered = buildThread(listOf(update(1, """{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Checking the tests"}}"""), chunk(2, "They pass.")))

        assertEquals(ThreadItem.Thought("acp-1", "Checking the tests", finished = false), thinking.single().let { it as ThreadItem.Thought }.copy(key = "acp-1"))
        assertEquals(true, (answered.first() as ThreadItem.Thought).finished)
        assertEquals("They pass.", (answered.last() as ThreadItem.AgentMessage).text)
    }

    @Test
    fun aCommandNamedOnlyByALaterUpdateShowsTheCommandAndItsUnfencedOutput() {
        val thread = buildThread(
            listOf(
                update(1, """{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Terminal","kind":"execute","status":"pending","rawInput":{}}"""),
                update(2, """{"sessionUpdate":"tool_call_update","toolCallId":"t1","title":"op whoami","rawInput":{"command":"op whoami"}}"""),
                update(3, """{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"failed","content":[{"type":"content","content":{"type":"text","text":"```console\nconnection reset\n```"}}]}"""),
            ),
        )
        val call = assertIs<ThreadItem.ToolCall>(thread.single())

        assertEquals("op whoami", call.argument)
        assertEquals("connection reset", call.output)
    }

    @Test
    fun consecutiveAgentChunksBecomeOneMessage() {
        val thread = buildThread(listOf(chunk(1, "Pinned "), chunk(2, "the version.")))

        assertEquals(1, thread.size)
        assertEquals("Pinned the version.", assertIs<ThreadItem.AgentMessage>(thread[0]).text)
    }

    @Test
    fun aToolCallBetweenChunksStartsANewAgentMessage() {
        val thread = buildThread(
            listOf(
                chunk(1, "Reading."),
                update(2, """{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Read package.json","kind":"read","status":"pending"}"""),
                chunk(3, "Done."),
            ),
        )

        assertEquals(listOf("AgentMessage", "ToolCall", "AgentMessage"), thread.map { it::class.simpleName })
    }

    @Test
    fun eventsAreRenderedInSequenceOrderWhateverTheArrivalOrder() {
        val thread = buildThread(
            listOf(
                event(2, EventKinds.TurnEnd, """{"stop_reason":"end_turn"}"""),
                event(1, EventKinds.UserMessage, """{"text":"Hello"}"""),
            ),
        )

        assertIs<ThreadItem.UserMessage>(thread[0])
        assertIs<ThreadItem.TurnEnd>(thread[1])
    }

    @Test
    fun aModelChangeBecomesAMarkerWhereTheConversationSwitched() {
        val thread = buildThread(
            listOf(
                event(1, EventKinds.UserMessage, """{"text":"Hello"}"""),
                event(2, EventKinds.ModelChanged, """{"provider_id":"p2","provider_name":"Anthropic","model":"claude-sonnet-5-5"}"""),
            ),
        )

        assertEquals(ThreadItem.ModelChanged("e2", "Anthropic", "claude-sonnet-5-5"), thread[1])
    }

    @Test
    fun toolCallUpdateChangesTheExistingCallInPlace() {
        val thread = buildThread(
            listOf(
                update(1, """{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Run tests","kind":"execute","status":"in_progress","rawInput":{"command":"pnpm -r test"}}"""),
                update(2, """{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"142 passed"}}]}"""),
            ),
        )

        val call = assertIs<ThreadItem.ToolCall>(thread.single())
        assertEquals(ToolStatus.Done, call.status)
        assertEquals("pnpm -r test", call.argument)
        assertEquals("142 passed", call.output)
    }

    @Test
    fun editToolCallCarriesADiffWithCounts() {
        val thread = buildThread(
            listOf(
                update(
                    1,
                    """{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Edit","kind":"edit","status":"completed",
                    "content":[{"type":"diff","path":"package.json","oldText":"a\nb\nc","newText":"a\nB\nc"}]}""",
                ),
            ),
        )

        val diff = assertIs<ThreadItem.ToolCall>(thread.single()).diff
        assertEquals("package.json", diff?.path)
        assertEquals(1, diff?.added)
        assertEquals(1, diff?.removed)
    }

    @Test
    fun diffMarksContextAddedAndRemovedLinesWithNumbers() {
        val lines = diffLines("one\ntwo\nthree", "one\n2\nthree")

        assertEquals(
            listOf(
                DiffLine(DiffLine.Kind.Context, 1, 1, "one"),
                DiffLine(DiffLine.Kind.Removed, 2, null, "two"),
                DiffLine(DiffLine.Kind.Added, null, 2, "2"),
                DiffLine(DiffLine.Kind.Context, 3, 3, "three"),
            ),
            lines,
        )
    }

    @Test
    fun aSmallEditToALongFileShowsOnlyTheEditWithItsRealLineNumbers() {
        val old = (1..419).joinToString("\n") { "line $it" }
        val new = old.replace("line 414", "line 414\nnew A\nnew B")

        val changed = diffLines(old, new).filter { it.kind != DiffLine.Kind.Context }

        assertEquals(listOf(DiffLine(DiffLine.Kind.Added, null, 415, "new A"), DiffLine(DiffLine.Kind.Added, null, 416, "new B")), changed)
        assertEquals(DiffLine(DiffLine.Kind.Context, 419, 421, "line 419"), diffLines(old, new).last())
    }

    @Test
    fun aNewFileIsAllAddedLines() {
        val lines = diffLines("", "a\nb")

        assertTrue(lines.all { it.kind == DiffLine.Kind.Added })
        assertEquals(2, lines.size)
    }

    @Test
    fun permissionRequestShowsOptionsUntilResolved() {
        val request = event(
            1,
            EventKinds.PermissionRequest,
            """{"request_id":"r1","toolCall":{"toolCallId":"t1","title":"Run rm -rf build"},
            "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"deny","name":"Reject","kind":"reject_once"}]}""",
        )

        val open = assertIs<ThreadItem.Permission>(buildThread(listOf(request)).single())
        assertEquals("Run rm -rf build", open.title)
        assertEquals(listOf("allow", "deny"), open.options.map { it.id })
        assertEquals(false, open.resolved)

        val resolved = buildThread(listOf(request, event(2, EventKinds.PermissionResolved, """{"request_id":"r1"}""")))
        assertEquals(true, assertIs<ThreadItem.Permission>(resolved.single()).resolved)
    }

    @Test
    fun planEntriesKeepTheirStatus() {
        val thread = buildThread(
            listOf(
                update(
                    1,
                    """{"sessionUpdate":"plan","entries":[{"content":"Bump","priority":"high","status":"completed"},{"content":"Test","priority":"high","status":"pending"}]}""",
                ),
            ),
        )

        val plan = assertIs<ThreadItem.Plan>(thread.single())
        assertEquals(listOf("Bump", "Test"), plan.entries.map { it.content })
    }

    @Test
    fun errorEventBecomesAFailureWithItsMessage() {
        val thread = buildThread(listOf(event(1, EventKinds.Error, """{"message":"Codex login on orion expired."}""")))

        assertEquals("Codex login on orion expired.", assertIs<ThreadItem.Failure>(thread.single()).message)
    }

    @Test
    fun workflowJournalLinesBuildOneCardThatFillsIn() {
        val thread = buildThread(
            listOf(
                event(1, EventKinds.Workflow, """{"run_id":"wf1","line":{"type":"launched"}}"""),
                event(2, EventKinds.Workflow, """{"run_id":"wf1","line":{"type":"started","key":"a","label":"New user","phase":"Build"}}"""),
                event(3, EventKinds.Workflow, """{"run_id":"wf1","line":{"type":"started","key":"b","label":"Machines","phase":"Build"}}"""),
                event(4, EventKinds.Workflow, """{"run_id":"wf1","line":{"type":"result","key":"a","result":"ok"}}"""),
            ),
        )

        val progress = assertIs<ThreadItem.Workflow>(thread.single()).progress
        assertEquals(2, progress.agents.size)
        assertEquals(1, progress.finishedCount)
        assertEquals(false, progress.complete)
        assertEquals(listOf("Build"), progress.phases)
    }
}

class SuggestedRepliesTest {
    @Test
    fun aNumberedChoiceAfterAQuestionBecomesOneReplyPerOption() {
        val answer = "Which should I do first?\n\n1. Open a merge request\n2. **Do the Android strings** too\n3. Run the e2e suite."

        assertEquals(listOf("Open a merge request", "Do the Android strings too", "Run the e2e suite"), suggestedReplies(answer))
    }

    @Test
    fun aBulletedChoiceFollowedByAClosingQuestionBecomesReplies() {
        val answer = "I can take either route:\n- Rewrite the parser\n- Patch the regex\n\nWhich do you prefer?"

        assertEquals(listOf("Rewrite the parser", "Patch the regex"), suggestedReplies(answer))
    }

    @Test
    fun aListIntroducedAsOptionsBecomesRepliesWithoutAQuestion() {
        assertEquals(listOf("Rewrite the parser", "Patch the regex"), suggestedReplies("There are two options:\n- Rewrite the parser\n- Patch the regex"))
    }

    @Test
    fun aSummaryListWithoutAQuestionSuggestsNothing() {
        assertEquals(emptyList(), suggestedReplies("Done. Changes:\n- Moved the strings\n- Updated three screens"))
    }

    @Test
    fun moreThanFourOptionsOrALongSentenceSuggestsNothing() {
        assertEquals(emptyList(), suggestedReplies("Pick one:\n- a\n- b\n- c\n- d\n- e"))
        assertEquals(emptyList(), suggestedReplies("Pick one:\n- ${"long ".repeat(30)}\n- short"))
    }

    @Test
    fun aYesOrNoQuestionOffersYesAndNo() {
        assertEquals(listOf("Yes", "No"), suggestedReplies("Tests pass. Want me to open a merge request?"))
    }

    @Test
    fun anOpenQuestionSuggestsNothing() {
        assertEquals(emptyList(), suggestedReplies("What colour should the button be?"))
    }

    @Test
    fun aTurnIsOpenFromTheMessageUntilItsTurnEnds() {
        val asked = listOf(event(1, EventKinds.UserMessage, """{"text":"go"}"""), chunk(2, "working"))
        val answered = asked + event(3, EventKinds.TurnEnd, """{"stop_reason":"end_turn"}""")
        val backgroundChatter = answered + chunk(4, "a subagent finished")

        assertTrue(turnIsOpen(asked))
        assertFalse(turnIsOpen(answered))
        assertFalse(turnIsOpen(backgroundChatter))
    }

    @Test
    fun aWorkflowsPlanListsItsPhasesBeforeTheyStartAndProgressUpdatesARunningAgent() {
        val events = listOf(
            event(1, EventKinds.Workflow, """{"run_id":"r","line":{"type":"plan","phases":["Write","Verify"]}}"""),
            event(2, EventKinds.Workflow, """{"run_id":"r","line":{"type":"started","key":"k1","agentId":"a1","label":"write:one","phase":"Write"}}"""),
            event(3, EventKinds.Workflow, """{"run_id":"r","line":{"type":"progress","key":"k1","agentId":"a1","tokens":1200,"turns":3,"elapsed_ms":4000}}"""),
        )

        val progress = buildThread(events).filterIsInstance<ThreadItem.Workflow>().single().progress

        assertEquals(listOf("Write", "Verify"), progress.phases)
        assertEquals(Triple(1200L, 3, 4000L), progress.agents.single().let { Triple(it.tokens, it.turns, it.elapsedMs) })
        assertFalse(progress.agents.single().finished)
    }
}
