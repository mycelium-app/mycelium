package mycelium

import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Project
import mycelium.state.worktreeFolder
import mycelium.state.worktreeHint
import kotlin.test.Test
import kotlin.test.assertEquals

class WorktreeTest {
    private val project = Project(id = "p", nodeId = "n", name = "harbour", path = "/home/you/harbour", createdAt = "2026-01-01T00:00:00Z")

    private fun conversation(cwd: String, branch: String?, updatedAt: String) =
        Conversation("c", "u", "n", "claude", cwd, "Title", ConversationStatus.Done, null, null, "opus", null, "2026-01-01T00:00:00Z", updatedAt, null, branch = branch)

    @Test
    fun folderIsTheProjectFolderWorktreesDirectoryNamedAfterTheBranchWithSlashesAsDashes() {
        assertEquals("harbour-worktrees/fix-restore-copy", worktreeFolder("/home/you/harbour", "fix/restore-copy"))
        assertEquals("harbour-worktrees/main", worktreeFolder("/home/you/harbour/", "main"))
    }

    @Test
    fun hintShowsTheFolderAndTheBranchTheProjectFolderHasCheckedOut() {
        val conversations = listOf(conversation("/home/you/harbour", "release", "2026-01-01T00:00:00Z"), conversation("/home/you/harbour", "main", "2026-02-01T00:00:00Z"), conversation("/elsewhere", "other", "2026-03-01T00:00:00Z"))

        assertEquals("Folder harbour-worktrees/fix-restore-copy, from main", worktreeHint(project, "fix/restore-copy", conversations))
        assertEquals("Names the branch and its folder, from main", worktreeHint(project, "  ", conversations))
    }

    @Test
    fun hintOmitsTheStartingBranchWhenNoConversationHasReportedOne() {
        assertEquals("Folder harbour-worktrees/x", worktreeHint(project, "x", emptyList()))
    }
}
