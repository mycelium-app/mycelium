package mycelium

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.Project
import mycelium.api.Task
import mycelium.api.TaskStatuses
import mycelium.api.Worktree
import mycelium.state.AppState
import mycelium.state.BoardFilter
import mycelium.state.Lane
import mycelium.state.boardCards
import mycelium.state.collapsedLanes
import mycelium.state.elapsed
import mycelium.state.pendingPermission
import mycelium.state.savedLanes
import mycelium.state.until
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.time.Instant

private fun conversation(id: String, status: ConversationStatus, cwd: String = "/other", agent: String = "claude", archived: Boolean = false, updatedAt: String = "2026-10-22T10:00:00Z") =
    Conversation(id, "u", "n1", agent, cwd, "Title $id", status, null, null, "opus", null, "2026-10-22T10:00:00Z", updatedAt, if (archived) "2026-10-22T11:00:00Z" else null)

private fun task(id: String, status: String, conversationId: String? = null, sourceTitle: String? = null, projectId: String? = null) =
    Task(id, "n1", "claude", "/work/app", projectId, null, "Title $id", null, "2026-10-22T10:00:00Z", status, emptyList(), null, null, sourceTitle, conversationId)

private val Project1 = Project("p1", "n1", "app", "/work/app", null, null, "2026-10-22T10:00:00Z")

private val Worktree1 = Worktree("w1", "p1", "feature/x", "/work/app-wt/feature-x", null, "2026-10-22T10:00:00Z")

private fun state(vararg conversations: Conversation, tasks: List<Task> = emptyList()) = AppState(
    conversations = conversations.associateBy { it.id },
    projects = mapOf("p1" to Project1),
    worktrees = mapOf("w1" to Worktree1),
    tasks = tasks.associateBy { it.id },
)

class BoardTest {
    @Test
    fun conversationsWithoutATaskWaitInClaimedUntilTheyAreDone() {
        val lanes = boardCards(
            state(
                conversation("a", ConversationStatus.Input),
                conversation("b", ConversationStatus.Working),
                conversation("c", ConversationStatus.Review),
                conversation("d", ConversationStatus.Failed),
                conversation("e", ConversationStatus.Done),
            ),
        )

        assertEquals(
            mapOf(Lane.Questions to emptyList(), Lane.Proposed to emptyList(), Lane.Open to emptyList(), Lane.Claimed to listOf("a", "b", "c", "d"), Lane.Done to listOf("e")),
            lanes.mapValues { (_, cards) -> cards.map { it.key }.sorted() },
        )
    }

    @Test
    fun archivedConversationsAreNotOnTheBoard() {
        val lanes = boardCards(state(conversation("a", ConversationStatus.Done, archived = true)))

        assertEquals(0, lanes.values.sumOf { it.size })
    }

    @Test
    fun openTasksSitInOpenWithTheirProject() {
        val task = Task("t1", "n1", "codex", "/work/app", "p1", "tidy", "Rebuild", null, "2026-10-22T10:00:00Z")
        val card = boardCards(state(tasks = listOf(task))).getValue(Lane.Open).single()

        assertEquals("Codex · app · tidy", card.details)
        assertEquals(task, card.task)
    }

    @Test
    fun everyTaskStatusLandsInItsLaneAndDeclinedOnesAreHiddenUntilAskedFor() {
        val tasks = listOf(
            task("q", TaskStatuses.Question),
            task("p", TaskStatuses.Proposed),
            task("o", TaskStatuses.Open),
            task("c", TaskStatuses.Claimed),
            task("d", TaskStatuses.Done),
            task("x", TaskStatuses.Declined),
        )

        assertEquals(
            mapOf(Lane.Questions to listOf("q"), Lane.Proposed to listOf("p"), Lane.Open to listOf("o"), Lane.Claimed to listOf("c"), Lane.Done to listOf("d")),
            boardCards(state(tasks = tasks)).mapValues { (_, cards) -> cards.map { it.key } },
        )
        assertEquals(listOf("d", "x"), boardCards(state(tasks = tasks), BoardFilter(showDeclined = true)).getValue(Lane.Done).map { it.key })
    }

    @Test
    fun aClaimedTaskCarriesItsConversationAndItsConversationIsNotListedAgain() {
        val claimed = task("t1", TaskStatuses.Claimed, conversationId = "a")
        val lanes = boardCards(state(conversation("a", ConversationStatus.Review), conversation("b", ConversationStatus.Working), tasks = listOf(claimed)))

        assertEquals(listOf("b", "t1"), lanes.getValue(Lane.Claimed).map { it.key }.sorted())
        assertEquals("a", lanes.getValue(Lane.Claimed).first { it.key == "t1" }.conversation?.id)
    }

    @Test
    fun questionsAndProposalsNameTheConversationTheyCameFrom() {
        val lanes = boardCards(state(tasks = listOf(task("q", TaskStatuses.Question, sourceTitle = "Release"), task("p", TaskStatuses.Proposed, sourceTitle = "Release", projectId = "p1"))))

        assertEquals("From Release", lanes.getValue(Lane.Questions).single().details)
        assertEquals("app · from Release", lanes.getValue(Lane.Proposed).single().details)
    }

    @Test
    fun aConversationInAWorktreeShowsItsProjectAndBranchAndOneInTheProjectFolderShowsNoBranch() {
        val lanes = boardCards(state(conversation("a", ConversationStatus.Working, "/work/app-wt/feature-x"), conversation("b", ConversationStatus.Review, "/work/app")))

        assertEquals("Claude Code · opus · app · feature/x", lanes.getValue(Lane.Claimed).first { it.key == "a" }.details)
        assertEquals("Claude Code · opus · app", lanes.getValue(Lane.Claimed).first { it.key == "b" }.details)
    }

    @Test
    fun theFilterNarrowsEveryLaneByTextProjectNodeAndAgent() {
        val everything = state(
            conversation("a", ConversationStatus.Working, "/work/app"),
            conversation("b", ConversationStatus.Review, "/elsewhere", agent = "codex"),
        )
        fun keys(filter: BoardFilter) = boardCards(everything, filter).values.flatten().map { it.key }

        assertEquals(listOf("a"), keys(BoardFilter(text = "title a")))
        assertEquals(listOf("a"), keys(BoardFilter(text = "APP")))
        assertEquals(listOf("a"), keys(BoardFilter(projectId = "p1")))
        assertEquals(listOf("b"), keys(BoardFilter(agent = "codex")))
        assertEquals(emptyList(), keys(BoardFilter(nodeId = "other")))
        assertEquals(2, keys(BoardFilter(nodeId = "n1")).size)
    }

    @Test
    fun theFiltersCombine() {
        val everything = state(conversation("a", ConversationStatus.Working, "/work/app"), conversation("b", ConversationStatus.Review, "/work/app", agent = "codex"))

        assertEquals(listOf("b"), boardCards(everything, BoardFilter(projectId = "p1", agent = "codex")).values.flatten().map { it.key })
    }

    @Test
    fun doneKeepsOnlyTheFiftyMostRecentConversations() {
        val done = (1..60).map { conversation("d$it", ConversationStatus.Done, updatedAt = "2026-10-22T10:${(it - 1).toString().padStart(2, '0')}:00Z") }
        val lane = boardCards(state(*done.toTypedArray())).getValue(Lane.Done)

        assertEquals(50, lane.size)
        assertEquals("d60", lane.first().key)
    }

    @Test
    fun doneIsCollapsedUntilTheChoiceIsSavedAndASavedChoiceIsRestored() {
        assertEquals(setOf(Lane.Done), collapsedLanes(null))
        assertEquals(emptySet(), collapsedLanes(savedLanes(emptySet())))
        assertEquals(setOf(Lane.Claimed, Lane.Done), collapsedLanes(savedLanes(setOf(Lane.Claimed, Lane.Done))))
        assertEquals(emptySet(), collapsedLanes("Nonsense"))
    }

    @Test
    fun theElapsedTimerCountsMinutesAndSecondsThenHoursAndMinutes() {
        val now = Instant.parse("2026-10-22T12:00:00Z")

        assertEquals("0m 07s", elapsed("2026-10-22T11:59:53Z", now))
        assertEquals("12m 05s", elapsed("2026-10-22T11:47:55Z", now))
        assertEquals("2h 03m", elapsed("2026-10-22T09:57:00Z", now))
        assertEquals("0m 00s", elapsed("2026-10-22T12:00:10Z", now))
    }

    @Test
    fun theTimeUntilTheNextRunIsInTheCoarsestWholeUnit() {
        val now = Instant.parse("2026-10-22T12:00:00Z")

        assertEquals("in under a minute", until("2026-10-22T12:00:30Z", now))
        assertEquals("in 11h", until("2026-10-22T23:30:00Z", now))
        assertEquals("in 3d", until("2026-10-25T12:00:00Z", now))
    }

    @Test
    fun theLatestUnansweredPermissionRequestIsWhatTheCardOffers() {
        fun request(seq: Long, id: String) = Event(
            "c", seq, EventKinds.PermissionRequest,
            JsonObject(mapOf("request_id" to JsonPrimitive(id), "toolCall" to JsonObject(mapOf("title" to JsonPrimitive("Run $id"))), "options" to JsonArray(emptyList()))),
        )
        val resolved = Event("c", 3, EventKinds.PermissionResolved, JsonObject(mapOf("request_id" to JsonPrimitive("r2"))))

        assertEquals("r2", pendingPermission(listOf(request(1, "r1"), request(2, "r2")))?.requestId)
        assertEquals("r1", pendingPermission(listOf(request(1, "r1"), request(2, "r2"), resolved))?.requestId)
        assertNull(pendingPermission(listOf(request(2, "r2"), resolved)))
    }

    @Test
    fun anOpenTaskWaitsOnlyOnTheTasksItDependsOnThatAreNotDone() {
        val first = task("a", "claimed")
        val finished = task("b", "done")
        val waiting = task("c", "open").copy(dependsOn = listOf("a", "b", "gone"))
        val tasks = listOf(first, finished, waiting).associateBy { it.id }

        assertEquals(listOf("a"), waitingOn(waiting, tasks).map { it.id })
        assertTrue(boardCards(state(tasks = tasks.values.toList()))[Lane.Open].orEmpty().single { it.key == "c" }.details.endsWith("waits on 1 task"))
        assertEquals(emptyList(), waitingOn(finished.copy(status = "open", dependsOn = listOf("b")), tasks))
    }
}
