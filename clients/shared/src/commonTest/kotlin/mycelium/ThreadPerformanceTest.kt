package mycelium

import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.MyceliumJson
import mycelium.state.ThreadBuilder
import mycelium.state.ThreadItem
import mycelium.state.buildThread
import mycelium.state.mergeEvents
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private fun event(seq: Long, kind: String, payload: String) = Event("c1", seq, kind, MyceliumJson.parseToJsonElement(payload))

private fun chunk(seq: Long) =
    event(seq, EventKinds.AcpUpdate, """{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"word $seq "}}}""")

private fun longConversation(turns: Int, chunksPerTurn: Int): List<Event> {
    var seq = 0L
    val events = mutableListOf<Event>()

    repeat(turns) { turn ->
        events += event(++seq, EventKinds.UserMessage, """{"text":"question $turn"}""")
        events += event(++seq, EventKinds.AcpUpdate, """{"update":{"sessionUpdate":"tool_call","toolCallId":"t$turn","title":"Read","kind":"read","status":"pending"}}""")
        events += event(++seq, EventKinds.AcpUpdate, """{"update":{"sessionUpdate":"tool_call_update","toolCallId":"t$turn","status":"completed"}}""")
        repeat(chunksPerTurn) { events += chunk(++seq) }
        events += event(++seq, EventKinds.TurnEnd, """{"stop_reason":"end_turn"}""")
    }

    return events
}

class ThreadPerformanceTest {
    @Test
    fun appendingChunksToALongConversationBuildsTheSameThreadAsARebuild() {
        val base = longConversation(turns = 200, chunksPerTurn = 100)
        val appended = (1..500).map { chunk(base.last().seq + it) }
        val builder = ThreadBuilder()

        var events = base
        var thread = builder.build(events)
        for (event in appended) {
            events = mergeEvents(events, listOf(event))
            thread = builder.build(events)
        }

        assertEquals(buildThread(events), thread)
        assertTrue(thread.last() is ThreadItem.AgentMessage)
    }

    @Test
    fun anIncrementalThreadEqualsAFullRebuildWhenEventsArriveOutOfOrderOrAreReplaced() {
        val events = longConversation(turns = 3, chunksPerTurn = 4)
        val builder = ThreadBuilder()

        assertEquals(buildThread(events.take(10)), builder.build(events.take(10)))
        assertEquals(buildThread(events), builder.build(events))

        val shuffled = events.take(5) + events.drop(8) + events.slice(5..7)

        assertEquals(buildThread(shuffled), builder.build(shuffled))

        val replaced = events.toMutableList().also { it[2] = it[2].copy(payload = MyceliumJson.parseToJsonElement("""{"update":{"sessionUpdate":"tool_call_update","toolCallId":"t0","status":"failed"}}""")) }

        assertEquals(buildThread(replaced), builder.build(replaced))
        assertEquals(buildThread(events.take(6)), builder.build(events.take(6)))
    }
}
