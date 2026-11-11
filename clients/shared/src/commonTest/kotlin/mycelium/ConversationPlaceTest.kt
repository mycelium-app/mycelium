package mycelium

import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.state.ConversationPlace
import mycelium.state.SubagentKind
import mycelium.state.chatSections
import mycelium.state.hasSession
import mycelium.state.hasWorkspace
import mycelium.state.place
import mycelium.state.readOnly
import mycelium.state.runsOnModel
import mycelium.state.sidebarSections
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlin.time.Instant

private val Now = Instant.parse("2026-10-23T12:00:00Z")

private fun conversation(
    id: String,
    agent: String,
    scratch: Boolean = false,
    cwd: String = "",
    projectId: String? = null,
    assistantId: String? = null,
    parentId: String? = null,
    sideKind: String? = null,
) = Conversation(
    id, "u", "n", agent, cwd, "Title $id", ConversationStatus.Done,
    scratch = scratch, projectId = projectId, assistantId = assistantId, parentId = parentId, sideKind = sideKind,
    createdAt = "2026-10-23T10:00:00Z", updatedAt = "2026-10-23T10:00:00Z",
)

private fun listedAsChat(conversation: Conversation) = chatSections(listOf(conversation), Now).any { section -> section.rows.any { it.id == conversation.id } }

private fun listedAsWorkspace(conversation: Conversation) = sidebarSections(listOf(conversation)).any { section -> section.rows.any { it.id == conversation.id } }

class ConversationPlaceTest {
    @Test
    fun aModelChatRunsOnAModelWithNoPlaceOrSessionAndListsAsAChat() {
        val chat = conversation("chat", "chat")

        assertTrue(chat.runsOnModel)
        assertEquals(ConversationPlace.None, chat.place)
        assertFalse(chat.readOnly)
        assertFalse(chat.hasWorkspace)
        assertFalse(chat.hasSession)
        assertTrue(listedAsChat(chat))
        assertFalse(listedAsWorkspace(chat))
    }

    @Test
    fun anAgentChatInAScratchFolderHasASessionButNoWorkspaceAndListsAsAChat() {
        val scratch = conversation("scratch", "claude", scratch = true, cwd = "/scratch/x")

        assertFalse(scratch.runsOnModel)
        assertEquals(ConversationPlace.Scratch, scratch.place)
        assertFalse(scratch.hasWorkspace)
        assertTrue(scratch.hasSession)
        assertTrue(listedAsChat(scratch))
        assertFalse(listedAsWorkspace(scratch))
    }

    @Test
    fun aFolderConversationWithAProjectHasAWorkspaceAndASessionAndListsAsAWorkspace() {
        val folder = conversation("work", "codex", cwd = "/work/app", projectId = "p1")

        assertFalse(folder.runsOnModel)
        assertEquals(ConversationPlace.Folder, folder.place)
        assertFalse(folder.readOnly)
        assertTrue(folder.hasWorkspace)
        assertTrue(folder.hasSession)
        assertFalse(listedAsChat(folder))
        assertTrue(listedAsWorkspace(folder))
    }

    @Test
    fun anAssistantSessionKeepsItsPlaceButListsInNeitherSidebarList() {
        val onModel = conversation("a-chat", "chat", assistantId = "a1")
        val inFolder = conversation("a-work", "claude", cwd = "/work/app", assistantId = "a1")

        assertEquals(ConversationPlace.None, onModel.place)
        assertEquals(ConversationPlace.Folder, inFolder.place)
        assertTrue(inFolder.hasWorkspace)
        assertFalse(listedAsChat(onModel))
        assertFalse(listedAsWorkspace(onModel))
        assertFalse(listedAsChat(inFolder))
        assertFalse(listedAsWorkspace(inFolder))
    }

    @Test
    fun anImportedSubagentIsReadOnlyAndAPlainSideChatIsNot() {
        val subagent = conversation("sub", "claude", cwd = "/work/app", parentId = "work", sideKind = SubagentKind)
        val side = conversation("side", "claude", cwd = "/work/app", parentId = "work", sideKind = "side")

        assertTrue(subagent.readOnly)
        assertFalse(side.readOnly)
        assertFalse(listedAsWorkspace(subagent))
    }

    @Test
    fun aSideChatListsWhereItsOwnPlaceSaysNotWhereItsParentIs() {
        val modelSide = conversation("m-side", "chat", parentId = "chat", sideKind = "side")
        val folderSide = conversation("f-side", "claude", cwd = "/work/app", parentId = "work", sideKind = "side")

        assertTrue(modelSide.runsOnModel)
        assertEquals(ConversationPlace.None, modelSide.place)
        assertTrue(listedAsChat(modelSide))
        assertEquals(ConversationPlace.Folder, folderSide.place)
        assertFalse(listedAsChat(folderSide))
    }
}
