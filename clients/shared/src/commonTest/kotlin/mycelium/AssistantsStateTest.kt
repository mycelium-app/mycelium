package mycelium

import mycelium.api.Assistant
import mycelium.api.Group
import mycelium.api.StreamMessage
import mycelium.state.AppState
import mycelium.state.reduce
import mycelium.state.AllGroups
import mycelium.state.AllStates
import mycelium.state.GroupByGroup
import mycelium.state.GroupByNone
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.state.assistantRows
import mycelium.state.harnessModels
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import mycelium.state.chatSections
import mycelium.state.pinnedChats
import mycelium.state.sidebarSections
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
    fun anIdleAssistantStaysInTheSidebarWhenThereIsRoom() {
        val all = listOf(
            assistant("older", active = "2026-10-20T10:00:00Z"),
            assistant("newer", active = "2026-10-23T10:00:00Z"),
            assistant("busy", state = "working", active = "2026-10-01T10:00:00Z"),
        )

        assertEquals(listOf("busy", "newer", "older"), sidebarAssistants(all, AllGroups).map { it.name })
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

    @Test
    fun assistantSessionsStayOutOfTheConversationSections() {
        val session = Conversation("s1", "u", "n", "chat", "", "Stand-up", ConversationStatus.Review, createdAt = "2026-10-23T10:00:00Z", updatedAt = "2026-10-23T10:00:00Z", assistantId = "a", pinnedAt = "2026-10-23T10:00:00Z")
        val chat = session.copy(id = "c1", assistantId = null)

        assertEquals(emptyList(), sidebarSections(listOf(session), AllGroups))
        assertEquals(listOf("c1"), pinnedChats(listOf(session, chat)).map { it.id })
        assertEquals(listOf("c1"), chatSections(listOf(session.copy(pinnedAt = null), chat.copy(pinnedAt = null))).flatMap { it.rows }.map { it.id })
    }

    @Test
    fun anAssistantFollowsItsCurrentSessionsStatus() {
        val working = Conversation("s1", "u", "n", "chat", "", "Stand-up", ConversationStatus.Working, createdAt = "2026-10-23T10:00:00Z", updatedAt = "2026-10-23T11:00:00Z", assistantId = "a")
        val before = AppState(assistants = mapOf("a" to assistant("a").copy(conversationId = "s1")))

        val after = reduce(before, StreamMessage.ConversationChanged(working))
        val finished = reduce(after, StreamMessage.ConversationChanged(working.copy(status = ConversationStatus.Review)))

        assertEquals("working", after.assistants.getValue("a").state)
        assertEquals("idle", finished.assistants.getValue("a").state)
    }

    @Test
    fun harnessModelsAreTheUnionOfWhatSessionsOfThatAgentOffered() {
        val options = { ids: List<String> -> buildJsonObject { put("models", buildJsonObject { put("availableModels", buildJsonArray { ids.forEach { id -> add(buildJsonObject { put("modelId", id); put("name", id) }) } }) }) } }
        val claude = Conversation("c1", "u", "n", "claude", "/w", "One", ConversationStatus.Review, createdAt = "2026-10-23T10:00:00Z", updatedAt = "2026-10-23T10:00:00Z", options = options(listOf("opus", "sonnet")))
        val other = claude.copy(id = "c2", options = options(listOf("sonnet", "haiku")))
        val codex = claude.copy(id = "c3", agent = "codex", options = options(listOf("gpt")))

        assertEquals(listOf("opus", "sonnet", "haiku"), harnessModels(listOf(claude, other, codex), "claude").map { it.id })
    }
}
