package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.ApiException
import mycelium.api.CreateScheduleRequest
import mycelium.api.CreateTaskRequest
import mycelium.api.EachRun
import mycelium.api.MyceliumApi
import mycelium.api.TaskStatuses
import mycelium.state.Store
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

private val Json = headersOf(HttpHeaders.ContentType, "application/json")

private fun taskJson(status: String, extra: String = "") =
    """{"id":"t1","node_id":"n1","agent":"claude","cwd":"/work/app","project_id":null,"branch":null,"title":"Tidy","prompt":null,"created_at":"2026-10-22T10:00:00Z","status":"$status","options":[]$extra}"""

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Tidy","status":"working","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val ScheduleJson = """{"id":"s1","node_id":"n1","agent":"claude","cwd":"/work/app","project_id":null,"prompt":"Go","cron":"0 6 * * 1","each_run":"new","paused":false,
"next_run_at":"2099-01-01T06:00:00Z","last_run_at":null,"last_conversation_id":null,"created_at":"2026-10-22T10:00:00Z"}"""

class TasksStoreTest {
    private fun store(scope: kotlinx.coroutines.CoroutineScope, requests: MutableList<Triple<String, String, String>> = mutableListOf(), answer: (String, HttpMethod) -> Pair<HttpStatusCode, String> = { path, method ->
        when {
            method == HttpMethod.Post && path.endsWith("/tasks/t1/approve") -> HttpStatusCode.OK to taskJson("open")
            method == HttpMethod.Post && path.endsWith("/tasks/t1/decline") -> HttpStatusCode.OK to taskJson("declined")
            method == HttpMethod.Post && path.endsWith("/tasks/t1/answer") -> HttpStatusCode.OK to taskJson("done", ",\"answer\":\"Wait\"")
            method == HttpMethod.Post && path.endsWith("/tasks/t1/done") -> HttpStatusCode.OK to taskJson("done")
            method == HttpMethod.Post && path.endsWith("/tasks/t1/start") -> HttpStatusCode.Created to ConversationJson
            method == HttpMethod.Post && path.endsWith("/tasks") -> HttpStatusCode.Created to taskJson("open")
            method == HttpMethod.Patch -> HttpStatusCode.OK to ScheduleJson
            method == HttpMethod.Post && path.endsWith("/schedules") -> HttpStatusCode.Created to ScheduleJson
            method == HttpMethod.Post && path.endsWith("/run") -> HttpStatusCode.Created to """{"id":"r1","schedule_id":"s1","ran_at":"2026-10-22T10:00:00Z","outcome":"started","conversation_id":"c1"}"""
            path.endsWith("/schedule-runs") -> HttpStatusCode.OK to """[{"id":"r1","schedule_id":"s1","ran_at":"2026-10-22T10:00:00Z","outcome":"started","conversation_id":"c1"}]"""
            path.endsWith("/schedules") -> HttpStatusCode.OK to "[$ScheduleJson]"
            path.endsWith("/tasks") -> HttpStatusCode.OK to "[${taskJson("open")}]"
            method == HttpMethod.Delete -> HttpStatusCode.NoContent to ""
            else -> HttpStatusCode.OK to "[]"
        }
    }): Store {
        val engine = MockEngine { request ->
            requests += Triple(request.method.value, request.url.encodedPath, String(request.body.toByteArray()))
            val (status, body) = answer(request.url.encodedPath, request.method)

            respond(body, status, Json)
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scope)
    }

    @Test
    fun refreshingTheBoardLoadsProjectsWorktreesAndBacklogTasks() = runTest {
        val store = store(backgroundScope)

        store.refreshBoard()

        assertEquals(setOf("t1"), store.state.value.tasks.keys)
    }

    @Test
    fun addingATaskPostsItAndKeepsItInTheBacklog() = runTest {
        val requests = mutableListOf<Triple<String, String, String>>()
        val store = store(backgroundScope, requests)

        store.createTask(CreateTaskRequest("n1", "claude", "Tidy", cwd = "/work/app"))

        assertEquals("""{"node_id":"n1","agent":"claude","title":"Tidy","cwd":"/work/app"}""", requests.single().third)
        assertEquals(setOf("t1"), store.state.value.tasks.keys)
    }

    @Test
    fun startingATaskMovesItFromTheBacklogToTheConversations() = runTest {
        val store = store(backgroundScope)
        store.refreshBoard()

        store.startTask(store.state.value.tasks.getValue("t1"))

        assertEquals(setOf("c1"), store.state.value.conversations.keys)
        assertEquals(TaskStatuses.Claimed, store.state.value.tasks.getValue("t1").status)
        assertEquals("c1", store.state.value.tasks.getValue("t1").conversationId)
    }

    @Test
    fun approvingDecliningAnsweringAndFinishingATaskReplaceItWithTheControlPlanesCopy() = runTest {
        val requests = mutableListOf<Triple<String, String, String>>()
        val store = store(backgroundScope, requests)
        store.refreshBoard()
        val task = store.state.value.tasks.getValue("t1")

        store.approveTask(task)
        assertEquals(TaskStatuses.Open, store.state.value.tasks.getValue("t1").status)

        store.declineTask(task)
        assertEquals(TaskStatuses.Declined, store.state.value.tasks.getValue("t1").status)

        store.answerTask(task, "Wait")
        assertEquals("Wait", store.state.value.tasks.getValue("t1").answer)
        assertEquals("""{"answer":"Wait"}""", requests.first { it.second.endsWith("/answer") }.third)

        store.finishTask(task)
        assertEquals(TaskStatuses.Done, store.state.value.tasks.getValue("t1").status)
    }

    @Test
    fun aTaskTheControlPlaneRefusesToStartStaysInTheBacklogWithItsMessage() = runTest {
        val store = store(backgroundScope) { path, method ->
            when {
                method == HttpMethod.Post -> HttpStatusCode.Conflict to """{"error":"node_offline","message":"This node is offline. Start it, then try again."}"""
                path.endsWith("/tasks") -> HttpStatusCode.OK to "[${taskJson("open")}]"
                else -> HttpStatusCode.OK to "[]"
            }
        }
        store.refreshBoard()

        val failure = assertFailsWith<ApiException> { store.startTask(store.state.value.tasks.getValue("t1")) }

        assertEquals("This node is offline. Start it, then try again.", failure.message)
        assertEquals(setOf("t1"), store.state.value.tasks.keys)
    }

    @Test
    fun deletingATaskRemovesItFromTheBacklog() = runTest {
        val store = store(backgroundScope)
        store.refreshBoard()

        store.deleteTask(store.state.value.tasks.getValue("t1"))

        assertEquals(emptySet(), store.state.value.tasks.keys)
    }

    @Test
    fun schedulesAndTheirRunsAreLoadedTogetherAndAScheduleIsCreatedPaused() = runTest {
        val requests = mutableListOf<Triple<String, String, String>>()
        val store = store(backgroundScope, requests)

        store.refreshSchedules()
        store.createSchedule(CreateScheduleRequest("n1", "claude", "Go", "0 6 * * 1", EachRun.New, cwd = "/work/app"))
        store.pauseSchedule(store.state.value.schedules.getValue("s1"), true)

        assertEquals(listOf("r1"), store.state.value.scheduleRuns.map { it.id })
        assertEquals("""{"node_id":"n1","agent":"claude","prompt":"Go","cron":"0 6 * * 1","each_run":"new","cwd":"/work/app"}""", requests.first { it.first == "POST" }.third)
        assertEquals("""{"paused":true}""", requests.first { it.first == "PATCH" }.third)
    }

    @Test
    fun runningAScheduleReloadsTheRunsAndDeletingItDropsItsRuns() = runTest {
        val requests = mutableListOf<Triple<String, String, String>>()
        val store = store(backgroundScope, requests)
        store.refreshSchedules()
        val schedule = store.state.value.schedules.getValue("s1")

        store.runSchedule(schedule)
        assertEquals(2, requests.count { it.second.endsWith("/schedule-runs") })

        store.deleteSchedule(schedule)
        assertEquals(emptySet(), store.state.value.schedules.keys)
        assertEquals(emptyList(), store.state.value.scheduleRuns)
    }
}
