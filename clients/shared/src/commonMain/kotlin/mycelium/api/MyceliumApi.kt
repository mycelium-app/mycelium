package mycelium.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.engine.HttpClientEngine
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.plugins.websocket.WebSockets
import io.ktor.client.plugins.websocket.webSocket
import io.ktor.client.request.HttpRequestBuilder
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.client.request.parameter
import io.ktor.client.request.patch
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.ktor.client.statement.HttpResponse
import io.ktor.http.ContentType
import io.ktor.http.HttpHeaders
import io.ktor.http.contentType
import io.ktor.http.isSuccess
import io.ktor.serialization.kotlinx.json.json
import io.ktor.websocket.Frame
import io.ktor.websocket.readText
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow

class ApiException(val status: Int, val code: String, override val message: String) : Exception(message)

sealed interface DevicePoll {
    data object Pending : DevicePoll

    data class Approved(val token: DeviceTokenResponse) : DevicePoll

    data class Rejected(val code: String, val message: String) : DevicePoll
}

class MyceliumApi(
    val baseUrl: String,
    private val clientToken: String? = null,
    engine: HttpClientEngine? = null,
) {
    private val client: HttpClient =
        if (engine == null) {
            HttpClient { configure() }
        } else {
            HttpClient(engine) { configure() }
        }

    private fun io.ktor.client.HttpClientConfig<*>.configure() {
        install(ContentNegotiation) { json(MyceliumJson) }
        install(WebSockets)
        expectSuccess = false
    }

    private val root = baseUrl.trimEnd('/')

    private fun HttpRequestBuilder.authorise() {
        if (clientToken != null) {
            val bearer = if (clientToken.startsWith("myc_client_")) clientToken else "myc_client_$clientToken"
            header(HttpHeaders.Authorization, "Bearer $bearer")
        }
    }

    private fun HttpRequestBuilder.json(body: Any) {
        contentType(ContentType.Application.Json)
        setBody(body)
    }

    private suspend fun HttpResponse.ensureSuccess(): HttpResponse {
        if (status.isSuccess()) {
            return this
        }

        val parsed = runCatching { body<ErrorBody>() }.getOrNull()
        throw ApiException(
            status = status.value,
            code = parsed?.error ?: "unknown",
            message = parsed?.message ?: "The server answered with status ${status.value}. Try again in a moment.",
        )
    }

    private suspend fun call(path: String, block: suspend (String) -> HttpResponse): HttpResponse =
        block("$root/api/v1$path").ensureSuccess()

    suspend fun me(): Me = call("/me") { client.get(it) { authorise() } }.body()

    suspend fun startDevice(request: DeviceRequest): DeviceResponse =
        call("/device") { client.post(it) { json(request) } }.body()

    suspend fun pollDevice(deviceCode: String): DevicePoll {
        val response = client.post("$root/api/v1/device/token") { json(DeviceTokenRequest(deviceCode)) }

        if (response.status.isSuccess()) {
            return DevicePoll.Approved(response.body())
        }

        val parsed = runCatching { response.body<ErrorBody>() }.getOrNull()
        val code = parsed?.error ?: "unknown"

        if (code == "authorization_pending") {
            return DevicePoll.Pending
        }

        return DevicePoll.Rejected(code, parsed?.message ?: "The sign-in request failed. Start again.")
    }

    suspend fun pendingDevice(userCode: String): PendingDevice =
        call("/device/$userCode") { client.get(it) { authorise() } }.body()

    suspend fun approveDevice(userCode: String) {
        call("/device/$userCode/approve") { client.post(it) { authorise() } }
    }

    suspend fun denyDevice(userCode: String) {
        call("/device/$userCode/deny") { client.post(it) { authorise() } }
    }

    suspend fun logout() {
        client.post("$root/auth/logout") { authorise() }
    }

    suspend fun sessions(): List<SessionSummary> = call("/sessions") { client.get(it) { authorise() } }.body()

    suspend fun endSession(id: String, confirmCurrent: Boolean = false) {
        call("/sessions/$id") { client.delete(it) { authorise(); if (confirmCurrent) parameter("confirm_current", "true") } }
    }

    suspend fun nodes(): List<NodeSummary> = call("/nodes") { client.get(it) { authorise() } }.body()

    suspend fun drainNode(id: String, draining: Boolean) {
        call("/nodes/$id/drain") { client.post(it) { authorise(); json(DrainRequest(draining)) } }
    }

    suspend fun updateNode(id: String, request: UpdateNodeRequest) {
        call("/nodes/$id") { client.patch(it) { authorise(); json(request) } }
    }

    suspend fun revokeNode(id: String) {
        call("/nodes/$id") { client.delete(it) { authorise() } }
    }

    suspend fun startLogin(nodeId: String, agent: String, method: String): StartLoginResponse =
        call("/nodes/$nodeId/logins") { client.post(it) { authorise(); json(StartLoginRequest(agent, method)) } }.body()

    suspend fun loginInput(loginId: String, text: String) {
        call("/logins/$loginId/input") { client.post(it) { authorise(); json(LoginInputRequest(text)) } }
    }

    suspend fun stopLogin(loginId: String) {
        call("/logins/$loginId") { client.delete(it) { authorise() } }
    }

    suspend fun startTerminal(conversationId: String, cols: Int, rows: Int): StartTerminalResponse =
        call("/conversations/$conversationId/terminal") { client.post(it) { authorise(); json(StartTerminalRequest(cols, rows)) } }.body()

    suspend fun terminalInput(terminalId: String, data: String) {
        call("/terminals/$terminalId/input") { client.post(it) { authorise(); json(TerminalInputRequest(data)) } }
    }

    suspend fun resizeTerminal(terminalId: String, cols: Int, rows: Int) {
        call("/terminals/$terminalId/resize") { client.post(it) { authorise(); json(ResizeTerminalRequest(cols, rows)) } }
    }

    suspend fun stopTerminal(terminalId: String) {
        call("/terminals/$terminalId") { client.delete(it) { authorise() } }
    }

    suspend fun memories(query: String = ""): List<Memory> =
        call("/memories") { client.get(it) { authorise(); if (query.isNotBlank()) parameter("query", query.trim()) } }.body()

    suspend fun updateMemory(id: String, text: String): Memory =
        call("/memories/$id") { client.patch(it) { authorise(); json(UpdateMemoryRequest(text)) } }.body()

    suspend fun deleteMemory(id: String) {
        call("/memories/$id") { client.delete(it) { authorise() } }
    }

    suspend fun forges(): List<Forge> = call("/forges") { client.get(it) { authorise() } }.body()

    suspend fun connectForge(request: ConnectForgeRequest): Forge =
        call("/forges") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun disconnectForge(id: String) {
        call("/forges/$id") { client.delete(it) { authorise() } }
    }

    suspend fun repositories(forge: String? = null, organisation: String? = null, query: String = ""): Repositories =
        call("/repositories") {
            client.get(it) {
                authorise()
                if (forge != null) parameter("forge", forge)
                if (organisation != null) parameter("organisation", organisation)
                if (query.isNotBlank()) parameter("q", query.trim())
            }
        }.body()

    suspend fun projects(): List<Project> = call("/projects") { client.get(it) { authorise() } }.body()

    suspend fun createProject(request: CreateProjectRequest): Project =
        call("/projects") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun worktrees(): List<Worktree> = call("/worktrees") { client.get(it) { authorise() } }.body()

    suspend fun tasks(): List<Task> = call("/tasks") { client.get(it) { authorise() } }.body()

    suspend fun createTask(request: CreateTaskRequest): Task =
        call("/tasks") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun startTask(id: String): Conversation =
        call("/tasks/$id/start") { client.post(it) { authorise() } }.body()

    suspend fun deleteTask(id: String) {
        call("/tasks/$id") { client.delete(it) { authorise() } }
    }

    suspend fun schedules(): List<Schedule> = call("/schedules") { client.get(it) { authorise() } }.body()

    suspend fun createSchedule(request: CreateScheduleRequest): Schedule =
        call("/schedules") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun pauseSchedule(id: String, paused: Boolean): Schedule =
        call("/schedules/$id") { client.patch(it) { authorise(); json(PatchScheduleRequest(paused)) } }.body()

    suspend fun runSchedule(id: String): ScheduleRun =
        call("/schedules/$id/run") { client.post(it) { authorise() } }.body()

    suspend fun deleteSchedule(id: String) {
        call("/schedules/$id") { client.delete(it) { authorise() } }
    }

    suspend fun scheduleRuns(): List<ScheduleRun> = call("/schedule-runs") { client.get(it) { authorise() } }.body()

    suspend fun issues(query: String = ""): Issues =
        call("/issues") { client.get(it) { authorise(); if (query.isNotBlank()) parameter("q", query.trim()) } }.body()

    suspend fun startWorkspace(projectId: String, number: Long, agent: String): Conversation =
        call("/projects/$projectId/issues/$number/workspace") { client.post(it) { authorise(); json(StartWorkspaceRequest(agent)) } }.body()

    suspend fun imports(): List<ImportStatus> = call("/imports") { client.get(it) { authorise() } }.body()

    suspend fun startImport(request: ImportRequest): ImportResult =
        call("/imports") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun indexing(): Indexing = call("/indexing") { client.get(it) { authorise() } }.body()

    suspend fun search(query: String, limit: Int = 20): List<SearchHit> =
        call("/search") { client.get(it) { authorise(); parameter("q", query); parameter("limit", limit) } }.body()

    suspend fun conversations(archived: Boolean = false): List<Conversation> =
        call("/conversations") { client.get(it) { authorise(); parameter("archived", archived) } }.body()

    suspend fun createConversation(request: CreateConversationRequest): Conversation =
        call("/conversations") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun conversation(id: String): Conversation = call("/conversations/$id") { client.get(it) { authorise() } }.body()

    suspend fun patchConversation(id: String, request: PatchConversationRequest): Conversation =
        call("/conversations/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun markOpened(id: String) {
        call("/conversations/$id/opened") { client.post(it) { authorise() } }
    }

    suspend fun files(id: String, query: String): List<String> =
        call("/conversations/$id/files") { client.get(it) { authorise(); parameter("query", query) } }.body()

    suspend fun gitStatus(id: String): GitStatus = call("/conversations/$id/git/status") { client.get(it) { authorise() } }.body()

    suspend fun gitLog(id: String, limit: Int = 50): List<GitCommit> =
        call("/conversations/$id/git/log") { client.get(it) { authorise(); parameter("limit", limit) } }.body()

    suspend fun gitDiff(id: String, path: String, staged: Boolean): GitDiff =
        call("/conversations/$id/git/diff") { client.get(it) { authorise(); parameter("path", path); parameter("staged", staged) } }.body()

    suspend fun gitStage(id: String, paths: List<String>) {
        call("/conversations/$id/git/stage") { client.post(it) { authorise(); json(GitPathsRequest(paths)) } }
    }

    suspend fun gitUnstage(id: String, paths: List<String>) {
        call("/conversations/$id/git/unstage") { client.post(it) { authorise(); json(GitPathsRequest(paths)) } }
    }

    suspend fun gitStageAll(id: String) {
        call("/conversations/$id/git/stage-all") { client.post(it) { authorise() } }
    }

    suspend fun gitCommit(id: String, message: String) {
        call("/conversations/$id/git/commit") { client.post(it) { authorise(); json(GitCommitRequest(message)) } }
    }

    suspend fun gitFetch(id: String) {
        call("/conversations/$id/git/fetch") { client.post(it) { authorise() } }
    }

    suspend fun gitUndo(id: String) {
        call("/conversations/$id/git/undo") { client.post(it) { authorise() } }
    }

    suspend fun events(id: String, after: Long = 0, limit: Int = 500): List<Event> =
        call("/conversations/$id/events") {
            client.get(it) {
                authorise()
                parameter("after", after)
                parameter("limit", limit)
            }
        }.body()

    suspend fun sendPrompt(id: String, text: String) {
        call("/conversations/$id/prompt") { client.post(it) { authorise(); json(PromptRequest(text)) } }
    }

    suspend fun queue(id: String): List<QueuedPrompt> = call("/conversations/$id/queue") { client.get(it) { authorise() } }.body()

    suspend fun editQueued(id: String, queueId: String, text: String) {
        call("/conversations/$id/queue/$queueId") { client.patch(it) { authorise(); json(PatchQueueRequest(text)) } }
    }

    suspend fun removeQueued(id: String, queueId: String) {
        call("/conversations/$id/queue/$queueId") { client.delete(it) { authorise() } }
    }

    suspend fun sendQueuedNow(id: String, queueId: String) {
        call("/conversations/$id/queue/$queueId/send-now") { client.post(it) { authorise() } }
    }

    suspend fun cancel(id: String) {
        call("/conversations/$id/cancel") { client.post(it) { authorise() } }
    }

    suspend fun answerPermission(id: String, requestId: String, optionId: String) {
        call("/conversations/$id/permission") {
            client.post(it) { authorise(); json(PermissionAnswer(requestId, optionId)) }
        }
    }

    suspend fun setConfig(id: String, request: ConfigRequest) {
        call("/conversations/$id/config") { client.post(it) { authorise(); json(request) } }
    }

    fun stream(): Flow<StreamMessage> = flow {
        val socketUrl = root.replaceFirst("http", "ws") + "/api/v1/stream"

        client.webSocket(socketUrl, request = { authorise() }) {
            for (frame in incoming) {
                if (frame is Frame.Text) {
                    val message = runCatching { parseStreamMessage(frame.readText()) }.getOrNull()

                    if (message != null) {
                        emit(message)
                    }
                }
            }
        }
    }

    fun close() {
        client.close()
    }
}
