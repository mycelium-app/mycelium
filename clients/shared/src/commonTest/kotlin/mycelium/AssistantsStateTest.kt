package mycelium

import mycelium.api.Assistant
import mycelium.api.Group
import mycelium.state.AllGroups
import mycelium.state.AllStates
import mycelium.state.GroupByGroup
import mycelium.state.GroupByNone
import mycelium.state.assistantRows
import mycelium.state.sidebarAssistants
import kotlin.test.Test
import kotlin.test.assertEquals

private fun assistant(name: String, state: String = "idle", pinned: Boolean = false, group: String? = null, description: String? = null, active: String = "2026-10-23T10:00:00Z", archived: Boolean = false) =
    Assistant(name, name, description, group, state = state, lastActiveAt = active, pinnedAt = "2026-10-01T00:00:00Z".takeIf { pinned }, archivedAt = "2026-10-02T00:00:00Z".takeIf { archived }, createdAt = "2026-09-01T00:00:00Z")

class AssistantsStateTest {
    @Test
    fun theSidebarShowsPinnedAndBusyAssistantsUpToFive() {
        val all = listOf(
            assistant("idle"),
            assistant("pinned", pinned = true, active = "2026-10-20T10:00:00Z"),
            assistant("working", state = "working", active = "2026-10-23T11:00:00Z"),
            assistant("input", state = "input", active = "2026-10-23T09:00:00Z"),
            assistant("archived", pinned = true, archived = true),
        ) + (1..5).map { assistant("extra$it", state = "working", active = "2026-10-22T0$it:00:00Z") }

        val shown = sidebarAssistants(all, AllGroups).map { it.name }

        assertEquals(listOf("pinned", "working", "input", "extra5", "extra4"), shown)
    }

    @Test
    fun searchMatchesDescriptions() {
        val all = listOf(assistant("Stand-up", description = "Daily summary"), assistant("War room", description = "Incidents"))

        val rows = assistantRows(all, emptyMap(), "summary", AllGroups, AllStates, GroupByNone).flatMap { it.rows }

        assertEquals(listOf("Stand-up"), rows.map { it.name })
    }

    @Test
    fun groupFilterScopesAssistants() {
        val all = listOf(assistant("a", group = "g1"), assistant("b", group = "g2"), assistant("c"))

        assertEquals(listOf("a"), sidebarAssistants(all.map { it.copy(pinnedAt = "2026-10-01T00:00:00Z") }, "g1").map { it.name })
        assertEquals(listOf("c"), assistantRows(all, emptyMap(), "", "none", AllStates, GroupByNone).flatMap { it.rows }.map { it.name })
    }

    @Test
    fun theTableGroupsAssistantsUnderTheirGroupName() {
        val groups = mapOf("g1" to Group("g1", "acme", createdAt = "2026-09-01T00:00:00Z"))
        val all = listOf(assistant("a", group = "g1"), assistant("b"))

        val sections = assistantRows(all, groups, "", AllGroups, AllStates, GroupByGroup)

        assertEquals(listOf("No group" to listOf("b"), "acme" to listOf("a")), sections.map { it.title to it.rows.map { row -> row.name } })
    }
}
