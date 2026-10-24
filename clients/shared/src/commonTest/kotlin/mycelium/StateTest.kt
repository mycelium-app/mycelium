package mycelium

import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.JsonObject
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Event
import mycelium.api.Project
import mycelium.api.Repository
import mycelium.api.StreamMessage
import mycelium.api.MyceliumJson
import mycelium.api.parseStreamMessage
import mycelium.state.AppState
import mycelium.state.TerminalChunkLimit
import mycelium.state.agentVersion
import mycelium.state.loginMethodName
import mycelium.state.projectsOfRepository
import mycelium.state.reduce
import mycelium.state.registeredFolders
import mycelium.state.relativeTime
import mycelium.state.sidebarSections
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.time.Instant

private fun conversation(
    id: String,
    status: ConversationStatus,
    updatedAt: String = "2026-10-22T12:00:00Z",
    archivedAt: String? = null,
) = Conversation(
    id = id,
    owner = "u",
    nodeId = "n",
    agent = "claude",
    cwd = "/home/me/midden",
    title = "Title $id",
    status = status,
    createdAt = "2026-10-22T10:00:00Z",
    updatedAt = updatedAt,
    archivedAt = archivedAt,
)

class StateTest {
    @Test
    fun sidebarOrdersByNeedsInputThenFailedThenReviewThenWorkingThenDone() {
        val sections = sidebarSections(
            listOf(
                conversation("done", ConversationStatus.Done),
                conversation("working", ConversationStatus.Working),
                conversation("review", ConversationStatus.Review),
                conversation("failed", ConversationStatus.Failed),
                conversation("input", ConversationStatus.Input),
            ),
        )

        assertEquals(
            listOf("Needs input", "Failed", "Ready to review", "Working", "Done"),
            sections.map { it.name },
        )
    }

    @Test
    fun aConversationWaitingForALimitToResetSitsUnderWorkingNotFailed() {
        val waiting = conversation("waiting", ConversationStatus.Failed).copy(resumeAt = "2026-10-22T13:00:00Z")
        val sections = sidebarSections(listOf(waiting, conversation("failed", ConversationStatus.Failed)))

        assertEquals(listOf("Failed" to listOf("failed"), "Working" to listOf("waiting")), sections.map { it.name to it.rows.map { row -> row.id } })
    }

    @Test
    fun sidebarOmitsEmptySectionsAndArchivedConversations() {
        val sections = sidebarSections(
            listOf(
                conversation("a", ConversationStatus.Working),
                conversation("gone", ConversationStatus.Done, archivedAt = "2026-10-22T11:00:00Z"),
            ),
        )

        assertEquals(listOf("Working"), sections.map { it.name })
    }

    @Test
    fun rowsInASectionAreNewestFirst() {
        val sections = sidebarSections(
            listOf(
                conversation("old", ConversationStatus.Working, "2026-10-22T09:00:00Z"),
                conversation("new", ConversationStatus.Working, "2026-10-22T11:00:00Z"),
            ),
        )

        assertEquals(listOf("new", "old"), sections.single().rows.map { it.id })
    }

    @Test
    fun streamConversationReplacesTheRow() {
        val before = AppState(conversations = mapOf("a" to conversation("a", ConversationStatus.Working)))
        val after = reduce(before, StreamMessage.ConversationChanged(conversation("a", ConversationStatus.Input)))

        assertEquals(ConversationStatus.Input, after.conversations.getValue("a").status)
    }

    @Test
    fun replayedEventsAreNotDuplicated() {
        val event = Event("a", 1, "user_message", JsonObject(emptyMap()))
        val once = reduce(AppState(openedConversations = setOf("a")), StreamMessage.EventAppended(event))
        val twice = reduce(once, StreamMessage.EventAppended(event))

        assertEquals(1, twice.events.getValue("a").size)
    }

    @Test
    fun eventsStreamedForAConversationThatWasNeverOpenedAreLeftForTheFirstFetch() {
        val live = Event("a", 7, "turn_end", JsonObject(emptyMap()))

        val state = reduce(AppState(), StreamMessage.EventAppended(live))

        assertNull(state.events["a"])
    }

    @Test
    fun loginOutputAccumulatesInOrder() {
        val state = listOf("Open ", "the link").fold(AppState()) { acc, part ->
            reduce(acc, StreamMessage.LoginOutput("l1", part))
        }

        assertEquals("Open the link", state.loginOutput.getValue("l1"))
    }

    @Test
    fun terminalOutputIsKeptPerTerminalAndOnlyUnseenChunksAreReturned() {
        val state = listOf("a" to "t1", "b" to "t2", "c" to "t1").fold(AppState()) { acc, (data, id) ->
            reduce(acc, StreamMessage.TerminalOutput(id, data))
        }

        assertEquals(listOf("a", "c"), state.terminalOutput.getValue("t1").chunks)
        assertEquals(listOf("b"), state.terminalOutput.getValue("t2").chunks)
        assertEquals(listOf("c"), state.terminalOutput.getValue("t1").unseen(1))
        assertEquals(emptyList(), state.terminalOutput.getValue("t1").unseen(2))
    }

    @Test
    fun terminalOutputIsBoundedAndKeepsTheNewestChunks() {
        val state = (1..TerminalChunkLimit + 10).fold(AppState()) { acc, number ->
            reduce(acc, StreamMessage.TerminalOutput("t1", "chunk$number"))
        }
        val feed = state.terminalOutput.getValue("t1")

        assertEquals(TerminalChunkLimit, feed.chunks.size)
        assertEquals(TerminalChunkLimit + 10, feed.received)
        assertEquals("chunk${TerminalChunkLimit + 10}", feed.chunks.last())
        assertEquals("chunk11", feed.chunks.first())
        assertEquals(TerminalChunkLimit, feed.unseen(0).size)
    }

    @Test
    fun anExitedTerminalIsRecorded() {
        val state = reduce(AppState(), StreamMessage.TerminalExited("t1"))

        assertEquals(setOf("t1"), state.exitedTerminals)
    }

    @Test
    fun terminalFramesParse() {
        assertEquals(StreamMessage.TerminalOutput("t1", "aGk="), parseStreamMessage("""{"type":"terminal_output","terminal_id":"t1","data":"aGk="}"""))
        assertEquals(StreamMessage.TerminalExited("t1"), parseStreamMessage("""{"type":"terminal_exited","terminal_id":"t1"}"""))
    }

    @Test
    fun streamFramesParseByTypeAndUnknownTypesAreIgnored() {
        val login = parseStreamMessage("""{"type":"login_output","login_id":"l1","data":"hi"}""")
        val finished = parseStreamMessage("""{"type":"login_finished","login_id":"l1","ok":true}""")
        val event = parseStreamMessage("""{"type":"event","conversation_id":"a","seq":3,"kind":"turn_end","payload":{"stop_reason":"end_turn"}}""")
        val unknown = parseStreamMessage("""{"type":"something_new"}""")

        assertEquals(StreamMessage.LoginOutput("l1", "hi"), login)
        assertEquals(StreamMessage.LoginFinished("l1", true), finished)
        assertEquals(3L, assertIs<StreamMessage.EventAppended>(event).event.seq)
        assertNull(unknown)
    }

    @Test
    fun streamConversationFrameParsesTheFlatRow() {
        val message = parseStreamMessage(
            """{"type":"conversation","id":"a","owner":"u","node_id":"n","agent":"codex","cwd":"/x","title":"T","status":"input",
            "acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}""",
        )

        val row = assertIs<StreamMessage.ConversationChanged>(message).conversation
        assertEquals(ConversationStatus.Input, row.status)
        assertEquals("codex", row.agent)
    }

    @Test
    fun aRemovedConversationFrameDropsTheConversationAndWhatWasLoadedForIt() {
        val row = assertIs<StreamMessage.ConversationChanged>(
            parseStreamMessage(
                """{"type":"conversation","id":"a","owner":"u","node_id":"n","agent":"codex","cwd":"/x","title":"T","status":"done",
                "acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":"2026-10-22T10:00:00Z"}""",
            ),
        )
        val state = reduce(AppState(), row).copy(openedConversations = setOf("a"), events = mapOf("a" to emptyList()))

        val after = reduce(state, assertIs<StreamMessage.ConversationRemoved>(parseStreamMessage("""{"type":"conversation_removed","conversation_id":"a"}""")))

        assertEquals(Triple(emptyMap(), emptyMap(), emptySet()), Triple(after.conversations, after.events, after.openedConversations))
    }

    @Test
    fun aRevokedNodeFrameRemovesTheNodeFromState() {
        val node = mycelium.api.NodeSummary(
            id = "n1", name = "box", hostname = "box", os = "linux", arch = "x86_64", version = "0.1.0", online = true, draining = false,
        )
        val state = reduce(AppState(), StreamMessage.NodeChanged(node))

        val message = parseStreamMessage("""{"type":"node_removed","node_id":"n1"}""")
        val after = reduce(state, assertIs<StreamMessage.NodeRemoved>(message))

        assertEquals(emptyMap(), after.nodes)
    }

    @Test
    fun relativeTimeShortensToTheLargestUnit() {
        val now = Instant.parse("2026-10-22T12:00:00Z")

        assertEquals("now", relativeTime("2026-10-22T11:59:30Z", now))
        assertEquals("4m", relativeTime("2026-10-22T11:56:00Z", now))
        assertEquals("3h", relativeTime("2026-10-22T09:00:00Z", now))
        assertEquals("2d", relativeTime("2026-10-20T12:00:00Z", now))
    }
}

class FormatTest {
    @Test
    fun loginMethodsReadTheSameWhicheverSeparatorTheNodeUses() {
        assertEquals("Subscription token", loginMethodName("oauth_token"))
        assertEquals("Subscription sign-in", loginMethodName("device_auth"))
        assertEquals("Subscription sign-in", loginMethodName("device-auth"))
        assertEquals("API key", loginMethodName("api_key"))
    }

    @Test
    fun anAgentVersionDropsThePackageName() {
        assertEquals("2.2.2", agentVersion("@agentclientprotocol/codex-acp 2.2.2"))
        assertEquals("0.94.0", agentVersion("0.94.0"))
    }
}

class SendingTest {
    private fun userMessage(text: String) = StreamMessage.EventAppended(Event("c1", 7, "user_message", MyceliumJson.parseToJsonElement("""{"text":"$text","prompt_id":"p"}""")))

    @Test
    fun aSentMessageStopsShowingAsSendingOnceTheServerRecordsIt() {
        val state = AppState(sending = mapOf("c1" to listOf("queued")))

        assertEquals(emptyList(), reduce(state, userMessage("queued")).sending["c1"])
    }

    @Test
    fun aMessageTheServerQueuedStopsShowingAsSendingAndTheQueueShowsIt() {
        val state = AppState(sending = mapOf("c1" to listOf("queued")))
        val queued = mycelium.api.QueuedPrompt("q1", "c1", 0, "queued", "2026-10-22T10:00:00Z")

        assertEquals(emptyList(), reduce(state, StreamMessage.QueueChanged("c1", listOf(queued))).sending["c1"])
    }
}

class HistoryTest {
    private fun conversation(id: String, status: ConversationStatus, archivedAt: String? = null) = Conversation(
        id = id, owner = "u", nodeId = "n", agent = "codex", cwd = "/work", title = id, status = status,
        createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z", archivedAt = archivedAt,
    )

    @Test
    fun allListsEveryConversationWhateverItsStatusAndDoneOnlyTheFinishedOnes() {
        val current = listOf(conversation("review", ConversationStatus.Review), conversation("done", ConversationStatus.Done), conversation("working", ConversationStatus.Working))
        val archived = listOf(conversation("old", ConversationStatus.Done, archivedAt = "2026-10-20T10:00:00Z"))

        assertEquals(setOf("review", "done", "working", "old"), mycelium.ui.pastConversations(current, archived, mycelium.ui.PastFilter.All, "").map { it.id }.toSet())
        assertEquals(listOf("done"), mycelium.ui.pastConversations(current, archived, mycelium.ui.PastFilter.Done, "").map { it.id })
    }

    @Test
    fun aConversationWithNoTurnsIsHiddenOnceUsageHasLoadedUnlessItsFirstTurnIsRunning() {
        val empty = conversation("empty", ConversationStatus.Done)
        val running = conversation("running", ConversationStatus.Working)
        val used = conversation("used", ConversationStatus.Done)
        val usage = mapOf("used" to mycelium.api.ActivityRow(key = "used", name = "", turns = 2))

        assertEquals(listOf("running", "used"), listOf(empty, running, used).filter { mycelium.ui.hasTurns(it, usage) }.map { it.id })
        assertEquals(listOf("empty", "running", "used"), listOf(empty, running, used).filter { mycelium.ui.hasTurns(it, null) }.map { it.id })
    }

    @Test
    fun subagentsAreLeftOutOfHistoryWhetherCurrentOrArchived() {
        val subagent = conversation("helper", ConversationStatus.Done).copy(parentId = "done", sideKind = "subagent")
        val current = listOf(conversation("done", ConversationStatus.Done), subagent)
        val archived = listOf(subagent.copy(id = "old-helper", archivedAt = "2026-10-20T10:00:00Z"))

        for (filter in mycelium.ui.PastFilter.entries) {
            assertEquals(listOf(), mycelium.ui.pastConversations(current, archived, filter, "").map { it.id }.filter { it.contains("helper") })
        }
    }
}

private fun project(id: String, node: String, forge: String?, repository: String?, name: String = id) =
    Project(id, node, name, "/p/$name", forge, repository, "2026-10-22T10:00:00Z")

class ProjectJoinTest {
    private val backups = Repository("f1", "acme-org", "acme-backups", "acme-org/acme-backups")

    @Test
    fun aRepositoryIsSetUpOnEveryNodeWhereAProjectMatchesItsForgeAndName() {
        val projects = listOf(
            project("a", "n1", "f1", "acme-org/acme-backups"),
            project("b", "n2", "f1", "acme-org/acme-backups"),
            project("c", "n1", "f2", "acme-org/acme-backups"),
            project("d", "n1", "f1", "acme-org/other"),
        )

        assertEquals(listOf("a", "b"), projectsOfRepository(projects, backups).map { it.id })
    }

    @Test
    fun registeredFoldersAreTheProjectsWithoutAForgeSortedByName() {
        val projects = listOf(project("a", "n1", "f1", "acme-org/acme-backups"), project("b", "n1", null, null, "zed"), project("c", "n2", null, null, "app"))

        assertEquals(listOf("app", "zed"), registeredFolders(projects).map { it.name })
    }

    private fun memoryMessage(id: String, text: String) =
        """{"type":"memory","id":"$id","text":"$text","scopes":["global"],"created_by":"laptop/Fix login","conversation_id":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

    @Test
    fun aStreamedMemoryIsKeptReplacedWhenEditedAndRemembersWhatWasRemoved() {
        val stored = reduce(AppState(), parseStreamMessage(memoryMessage("m1", "Staging listens on 5433"))!!)
        val edited = reduce(stored, parseStreamMessage(memoryMessage("m1", "Staging listens on 5434"))!!)

        assertEquals("Staging listens on 5434", edited.memories.getValue("m1").text)

        val removed = reduce(edited, parseStreamMessage("""{"type":"memory_removed","memory_id":"m1"}""")!!)

        assertEquals(Triple(emptySet<String>(), setOf("m1"), 3), Triple(removed.memories.keys, removed.removedMemories, removed.memoryRevision))
    }
}

class AgentActiveTest {
    private val base = """"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work","title":"t","status":"working","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z""""

    @Test
    fun aConversationIsAgentActiveOnlyWhenTheServerSaysSo() {
        assertEquals(true, MyceliumJson.decodeFromString<Conversation>("""{$base,"agent_active":true}""").agentActive)
        assertEquals(false, MyceliumJson.decodeFromString<Conversation>("""{$base}""").agentActive)
    }
}
