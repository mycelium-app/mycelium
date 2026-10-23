package mycelium

import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.state.chatSections
import mycelium.state.isChat
import mycelium.state.sidebarSections
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.time.Instant

private val Now = Instant.parse("2026-10-23T12:00:00Z")

private fun conversation(id: String, agent: String = "chat", updatedAt: String = "2026-10-23T10:00:00Z", archivedAt: String? = null, parentId: String? = null) =
    Conversation(id, "u", "n", agent, "", "Title $id", ConversationStatus.Done, createdAt = updatedAt, updatedAt = updatedAt, archivedAt = archivedAt, parentId = parentId, sideKind = parentId?.let { "side" })

class ChatsTest {
    @Test
    fun chatsGroupIntoTodayThisWeekAndOlderNewestFirst() {
        val all = listOf(
            conversation("old", updatedAt = "2026-10-01T10:00:00Z"),
            conversation("week", updatedAt = "2026-10-20T10:00:00Z"),
            conversation("early", updatedAt = "2026-10-23T01:00:00Z"),
            conversation("late", updatedAt = "2026-10-23T11:00:00Z"),
        )

        val sections = chatSections(all, Now)

        assertEquals(listOf("Today", "This week", "Older"), sections.map { it.name })
        assertEquals(listOf("late", "early"), sections[0].rows.map { it.id })
        assertEquals(listOf("week"), sections[1].rows.map { it.id })
        assertEquals(listOf("old"), sections[2].rows.map { it.id })
    }

    @Test
    fun emptyGroupsAreLeftOut() {
        assertEquals(listOf("Older"), chatSections(listOf(conversation("old", updatedAt = "2026-01-01T00:00:00Z")), Now).map { it.name })
    }

    @Test
    fun archivedChatsAndWorkspaceConversationsAreNotListed() {
        val all = listOf(conversation("archived", archivedAt = "2026-10-23T11:00:00Z"), conversation("work", agent = "claude"))

        assertTrue(chatSections(all, Now).isEmpty())
    }

    @Test
    fun aSideChatLeavesTheGroupsAndSitsUnderItsParent() {
        val all = listOf(conversation("parent"), conversation("side", parentId = "parent"))

        assertEquals(listOf("parent"), chatSections(all, Now).single().rows.map { it.id })
    }

    @Test
    fun chatsNeverAppearAmongTheWorkspaceConversations() {
        val all = listOf(conversation("chat"), conversation("work", agent = "claude"))

        assertEquals(listOf("work"), sidebarSections(all).flatMap { it.rows }.map { it.id })
        assertTrue(all[0].isChat)
    }
}
