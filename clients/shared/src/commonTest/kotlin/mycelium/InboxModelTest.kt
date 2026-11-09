package mycelium

import mycelium.api.Assistant
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.state.AppState
import mycelium.state.VoiceAssistantKey
import mycelium.ui.inboxItems
import kotlin.test.Test
import kotlin.test.assertEquals

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
}
