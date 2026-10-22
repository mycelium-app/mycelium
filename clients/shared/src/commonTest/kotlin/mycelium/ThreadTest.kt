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
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

private fun json(text: String): JsonElement = MyceliumJson.parseToJsonElement(text)

private fun event(seq: Long, kind: String, payload: String) = Event("c1", seq, kind, json(payload))

private fun update(seq: Long, body: String) = event(seq, EventKinds.AcpUpdate, """{"sessionId":"s","update":$body}""")

private fun chunk(seq: Long, text: String) =
    update(seq, """{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"$text"}}""")

class ThreadTest {
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
