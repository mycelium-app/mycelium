package mycelium

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import mycelium.state.PinDismissals
import mycelium.state.ThreadItem
import mycelium.state.WorkflowAgent
import mycelium.state.WorkflowProgress
import mycelium.state.applyWorkflowJournal
import mycelium.state.pinnedCards
import mycelium.state.workflowPinKey
import mycelium.state.workflowStats
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

private fun card(id: String, heading: String, sticky: Boolean = true) =
    "```mycelium-ui\n{\"type\":\"heading\",\"text\":\"$heading\",\"id\":\"$id\",\"sticky\":$sticky}\n```"

private fun message(key: String, text: String) = ThreadItem.AgentMessage(key, text)

class PinsTest {
    @Test
    fun aStickyBlockIsPinned() {
        val pinned = pinnedCards(listOf(message("a", card("plan", "Plan"))), emptySet())

        assertEquals(listOf("Plan"), pinned.map { it.label })
    }

    @Test
    fun aBlockThatIsNotStickyIsNotPinned() {
        assertEquals(emptyList(), pinnedCards(listOf(message("a", card("plan", "Plan", sticky = false))), emptySet()))
    }

    @Test
    fun aLaterBlockWithTheSameIdReplacesThePinnedOneInPlace() {
        val items = listOf(message("a", card("one", "One")), message("b", card("two", "Two")), message("c", card("one", "One again", sticky = false)))

        assertEquals(listOf("One again", "Two"), pinnedCards(items, emptySet()).map { it.label })
    }

    @Test
    fun aFourthPinnedBlockUnpinsTheOldest() {
        val items = listOf("a", "b", "c", "d").map { message(it, card(it, it.uppercase())) }

        assertEquals(listOf("B", "C", "D"), pinnedCards(items, emptySet()).map { it.label })
    }

    @Test
    fun aDismissedPinStaysGoneEvenWhenItsBlockIsReplaced() {
        val items = listOf(message("a", card("plan", "Plan")), message("b", card("plan", "Plan v2")))
        val dismissed = setOf(pinnedCards(items, emptySet()).single().key)

        assertEquals(emptyList(), pinnedCards(items, dismissed))
    }

    @Test
    fun aWorkflowIsPinnedOnlyWhileItRunsInAWorkingConversation() {
        val running = WorkflowProgress("run", listOf(WorkflowAgent("a", "Agent", "Build", finished = false)))
        val finished = WorkflowProgress("run", listOf(WorkflowAgent("a", "Agent", "Build", finished = true)))

        assertEquals(listOf(workflowPinKey("run")), pinnedCards(listOf(ThreadItem.Workflow("w", running)), emptySet(), working = true).map { it.key })
        assertEquals(emptyList(), pinnedCards(listOf(ThreadItem.Workflow("w", finished)), emptySet(), working = true))
        assertEquals(emptyList(), pinnedCards(listOf(ThreadItem.Workflow("w", running)), emptySet(), working = false))
        assertEquals(emptyList(), pinnedCards(listOf(ThreadItem.Workflow("w", running), ThreadItem.Workflow("w2", finished)), emptySet(), working = true))
    }

    @Test
    fun dismissalsAreRememberedPerConversation() {
        val dismissals = PinDismissals()

        dismissals.dismiss("one", "ui:plan")

        assertEquals(setOf("ui:plan"), dismissals["one"])
        assertEquals(emptySet(), dismissals["two"])
    }

    @Test
    fun agentStatsComeFromTheJournalOnlyWhenItCarriesThem() {
        fun line(type: String, build: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit): JsonObject = buildJsonObject {
            put("type", type)
            put("key", "a")
            build()
        }

        var agents = applyWorkflowJournal(emptyList(), line("started") { put("label", "Agent") })

        assertNull(workflowStats(agents.single()))

        agents = applyWorkflowJournal(agents, line("result") {
            put("tokens", 12_345)
            put("turns", 4)
            put("elapsed_ms", 125_000)
        })

        assertEquals("12.3k tokens · 4 turns · 2m 05s", workflowStats(agents.single()))
        assertEquals("12.3k tokens · 4 turns · 2m 05s", workflowStats(WorkflowProgress("run", agents).totals))
    }
}
