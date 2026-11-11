package mycelium

import mycelium.api.Assistant
import mycelium.api.ApiAccessRequest
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.InstructionProposal
import mycelium.api.MessageProposal
import mycelium.api.StartProposal
import mycelium.api.StartProposalStep
import mycelium.state.AppState
import mycelium.state.VoiceAssistantKey
import mycelium.ui.inboxItems
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class InboxModelTest {
    private fun waiting(id: String, assistantId: String) = Conversation(
        id = id, owner = "u", nodeId = "n", agent = "claude", cwd = "/work", title = id, status = ConversationStatus.Input,
        createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z", assistantId = assistantId,
    )

    @Test
    fun aVoiceConversationWaitingForAReplyIsNotInNeedsYouWhileAnotherAssistantsIs() {
        val state = AppState(
            conversations = listOf(waiting("voice-chat", "voice"), waiting("archivist-chat", "archivist")).associateBy { it.id },
            assistants = listOf(
                Assistant("voice", "Voice", builtinKey = VoiceAssistantKey, createdAt = "2026-10-01T00:00:00Z"),
                Assistant("archivist", "Archivist", createdAt = "2026-10-01T00:00:00Z"),
            ).associateBy { it.id },
        )

        assertEquals(listOf("archivist-chat"), inboxItems(state).map { it.conversation.id })
    }

    private fun conversation(id: String, status: ConversationStatus, managedBy: String? = null, archivedAt: String? = null) = Conversation(
        id = id, owner = "u", nodeId = "n", agent = "claude", cwd = "/work", title = id, status = status, managedBy = managedBy, archivedAt = archivedAt,
        createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z",
    )

    private fun startProposal(id: String, conversationId: String, state: String = "proposed") = StartProposal(
        id = id, conversationId = conversationId, kind = "subagent", title = "Review the schema", steps = listOf(StartProposalStep("review", "Review the schema")),
        state = state, createdAt = "2026-10-22T10:01:00Z",
    )

    private fun messageProposal(id: String, conversationId: String, state: String = "proposed") = MessageProposal(
        id = id, conversationId = conversationId, targetId = "t", targetTitle = "Target", targetNode = "atlas", message = "Hello", reason = "Because", state = state, createdAt = "2026-10-22T10:01:00Z",
    )

    @Test
    fun aStartProposalWaitingInAWorkingConversationIsInNeedsYouUntilItIsDecided() {
        val chats = mapOf("w" to conversation("w", ConversationStatus.Working))

        assertEquals(listOf("w/start-p1"), inboxItems(AppState(conversations = chats, startProposals = mapOf("p1" to startProposal("p1", "w")))).map { it.key })
        assertTrue(inboxItems(AppState(conversations = chats, startProposals = mapOf("p1" to startProposal("p1", "w", "started")))).isEmpty())
        assertTrue(inboxItems(AppState(conversations = chats, startProposals = mapOf("p1" to startProposal("p1", "w", "declined")))).isEmpty())
    }

    @Test
    fun aMessageProposalWaitingInAnAssistantManagedReviewConversationIsInNeedsYou() {
        val chats = mapOf("r" to conversation("r", ConversationStatus.Review, managedBy = "assistant"))

        assertEquals(listOf("r/message-m1"), inboxItems(AppState(conversations = chats, messageProposals = mapOf("m1" to messageProposal("m1", "r")))).map { it.key })
        assertTrue(inboxItems(AppState(conversations = chats, messageProposals = mapOf("m1" to messageProposal("m1", "r", "sent")))).isEmpty())
    }

    @Test
    fun aProposalOfAnArchivedOrUnknownConversationIsNotInNeedsYou() {
        val chats = mapOf("a" to conversation("a", ConversationStatus.Working, archivedAt = "2026-10-22T11:00:00Z"))
        val proposals = mapOf("p1" to startProposal("p1", "a"), "p2" to startProposal("p2", "gone"))

        assertTrue(inboxItems(AppState(conversations = chats, startProposals = proposals)).isEmpty())
    }

    @Test
    fun anApiAccessRequestAndAnInstructionProposalWaitingInAWorkingConversationAreInNeedsYou() {
        val chats = mapOf("w" to conversation("w", ConversationStatus.Working))
        val access = ApiAccessRequest("a1", "w", listOf("read"), 1, "Needed", "requested", "2026-10-22T10:02:00Z")
        val edit = InstructionProposal("i1", "w", "w", "global", "Global", "rev", "old", "new", "Tidy", "proposed", createdAt = "2026-10-22T10:03:00Z")

        assertEquals(listOf("w/instructions-i1", "w/api-access-a1"), inboxItems(AppState(conversations = chats, apiAccessRequests = mapOf("a1" to access), instructionProposals = mapOf("i1" to edit))).map { it.key })
    }

    @Test
    fun aProposalIsListedOnceWhenItsConversationAlsoWaitsOnThePerson() {
        val chats = mapOf("i" to conversation("i", ConversationStatus.Input))
        val items = inboxItems(AppState(conversations = chats, startProposals = mapOf("p1" to startProposal("p1", "i"))))

        assertEquals(listOf("i/start-p1"), items.map { it.key })
    }
}
