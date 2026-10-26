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
import io.ktor.client.request.put
import io.ktor.client.request.setBody
import io.ktor.http.encodeURLPathPart
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
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

private const val OlderUnknownRouteMessage = "That address was not found. Check the address and try again."
private const val UnknownRouteMessage = "This control plane does not offer that yet. Update it to the latest version of Mycelium, then try again."

class ApiException(val status: Int, val code: String, override val message: String) : Exception(message)

sealed interface DevicePoll {
    data object Pending : DevicePoll

    data class Approved(val token: DeviceTokenResponse) : DevicePoll

    data class Rejected(val code: String, val message: String) : DevicePoll
}

private const val StreamPingMillis = 20_000L

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
        install(WebSockets) { pingIntervalMillis = StreamPingMillis }
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

        if (body is JsonElement) {
            setBody(body.toString())
        } else {
            setBody(body)
        }
    }

    private suspend fun HttpResponse.ensureSuccess(): HttpResponse {
        if (status.isSuccess()) {
            return this
        }

        val parsed = runCatching { body<ErrorBody>() }.getOrNull()
        val unknownRoute = parsed?.error == "unknown_route" || (parsed?.error == "not_found" && parsed.message == OlderUnknownRouteMessage)

        throw ApiException(
            status = status.value,
            code = if (unknownRoute) "unknown_route" else parsed?.error ?: "unknown",
            message = if (unknownRoute) UnknownRouteMessage else parsed?.message ?: "The server answered with status ${status.value}. Try again in a moment.",
        )
    }

    private suspend fun call(path: String, block: suspend (String) -> HttpResponse): HttpResponse =
        block("$root/api/v1$path").ensureSuccess()

    suspend fun workflows(): List<JsonObject> = call("/workflows") { client.get(it) { authorise() } }.body()

    suspend fun createWorkflow(plan: JsonObject): JsonObject = call("/workflows") { client.post(it) { authorise(); json(plan) } }.body()

    suspend fun controlWorkflow(id: String, action: JsonObject): JsonObject = call("/workflows/$id/control") { client.post(it) { authorise(); json(action) } }.body()

    suspend fun me(): Me = call("/me") { client.get(it) { authorise() } }.body()

    suspend fun version(): String = call("/version") { client.get(it) }.body<ServerVersion>().version

    suspend fun startDevice(request: DeviceRequest, returnTo: String? = null): DeviceResponse =
        call("/device") { client.post(it) { json(request); returnTo?.let { target -> parameter("return_to", target) } } }.body()

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

    suspend fun activity(range: String, nodeId: String? = null): Activity =
        call("/activity") { client.get(it) { authorise(); parameter("range", range); if (nodeId != null) parameter("node_id", nodeId) } }.body()

    suspend fun usageOf(id: String): ConversationUsage = call("/conversations/$id/usage") { client.get(it) { authorise() } }.body()

    suspend fun conversationUsage(): List<ActivityRow> = call("/activity/conversations") { client.get(it) { authorise() } }.body()

    suspend fun nodes(): List<NodeSummary> = call("/nodes") { client.get(it) { authorise() } }.body()

    suspend fun drainNode(id: String, draining: Boolean) {
        call("/nodes/$id/drain") { client.post(it) { authorise(); json(DrainRequest(draining)) } }
    }

    suspend fun updateNodeSoftware(id: String) {
        call("/nodes/$id/update") { client.post(it) { authorise() } }
    }

    suspend fun directories(nodeId: String, path: String): Directories =
        call("/nodes/$nodeId/directories") { client.get(it) { authorise(); parameter("path", path) } }.body()

    suspend fun watchNode(id: String): NodeResources = call("/nodes/$id/watch") { client.post(it) { authorise() } }.body()

    suspend fun useReset(request: UseResetRequest): UseResetResponse = call("/usage/resets") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun stopProcess(nodeId: String, pid: Int) {
        call("/nodes/$nodeId/processes/$pid/stop") { client.post(it) { authorise() } }
    }

    suspend fun updateNode(id: String, request: UpdateNodeRequest) {
        call("/nodes/$id") { client.patch(it) { authorise(); json(request) } }
    }

    suspend fun revokeNode(id: String) {
        call("/nodes/$id") { client.delete(it) { authorise() } }
    }

    suspend fun takeOverInstructions(nodeId: String) {
        call("/nodes/$nodeId/instructions/take-over") { client.post(it) { authorise() } }
    }

    suspend fun startLogin(nodeId: String, agent: String, method: String, account: String? = null): StartLoginResponse =
        call("/nodes/$nodeId/logins") { client.post(it) { authorise(); json(StartLoginRequest(agent, method, account)) } }.body()

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

    suspend fun modelProviders(): List<ModelProvider> = call("/model-providers") { client.get(it) { authorise() } }.body()

    suspend fun modelProviderUsage(): List<ModelProviderUsage> = call("/model-provider-usage") { client.get(it) { authorise() } }.body()

    suspend fun createModelProvider(request: CreateModelProviderRequest): ModelProvider =
        call("/model-providers") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun updateModelProvider(id: String, request: UpdateModelProviderRequest): ModelProvider =
        call("/model-providers/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun setFavouriteModel(id: String, model: String, favourite: Boolean): ModelProvider =
        call("/model-providers/$id/favourites") { client.put(it) { authorise(); json(SetFavouriteModelRequest(model, favourite)) } }.body()

    suspend fun refreshModelProvider(id: String): ModelProvider =
        call("/model-providers/$id/refresh") { client.post(it) { authorise() } }.body()

    suspend fun deleteModelProvider(id: String) {
        call("/model-providers/$id") { client.delete(it) { authorise() } }
    }

    suspend fun chatToolServers(): List<ChatToolServer> = call("/chat-tool-servers") { client.get(it) { authorise() } }.body()

    suspend fun createChatToolServer(request: CreateChatToolServerRequest): ChatToolServer =
        call("/chat-tool-servers") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun startChatToolServerOAuth(request: StartChatToolServerOAuthRequest): OAuthStart =
        call("/chat-tool-servers/oauth") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun patchChatToolServer(id: String, request: PatchChatToolServerRequest): ChatToolServer =
        call("/chat-tool-servers/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun refreshChatToolServer(id: String): ChatToolServer =
        call("/chat-tool-servers/$id/refresh") { client.post(it) { authorise() } }.body()

    suspend fun deleteChatToolServer(id: String) {
        call("/chat-tool-servers/$id") { client.delete(it) { authorise() } }
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

    suspend fun assistants(archived: Boolean = false): List<Assistant> =
        call("/assistants") { client.get(it) { authorise(); if (archived) parameter("archived", true) } }.body()

    suspend fun assistant(id: String): Assistant = call("/assistants/$id") { client.get(it) { authorise() } }.body()

    suspend fun createAssistant(request: CreateAssistantRequest): Assistant =
        call("/assistants") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun patchAssistant(id: String, request: PatchAssistantRequest): Assistant =
        call("/assistants/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun assistantFiles(id: String): List<AssistantFile> = call("/assistants/$id/files") { client.get(it) { authorise() } }.body()

    suspend fun assistantFile(id: String, path: String): String = call("/assistants/$id/files/${filePath(path)}") { client.get(it) { authorise() } }.body()

    suspend fun saveAssistantFile(id: String, path: String, text: String): AssistantFile =
        call("/assistants/$id/files/${filePath(path)}") { client.put(it) { authorise(); contentType(ContentType.Text.Plain); setBody(text) } }.body()

    suspend fun deleteAssistantFile(id: String, path: String) {
        call("/assistants/$id/files/${filePath(path)}") { client.delete(it) { authorise() } }
    }

    suspend fun setAssistantBackup(id: String, request: SetAssistantBackupRequest): Assistant =
        call("/assistants/$id/backup") { client.put(it) { authorise(); json(request) } }.body()

    suspend fun clearAssistantBackup(id: String) {
        call("/assistants/$id/backup") { client.delete(it) { authorise() } }
    }

    suspend fun pullAssistantBackup(id: String): Assistant = call("/assistants/$id/backup/pull") { client.post(it) { authorise() } }.body()

    suspend fun assistantSecrets(id: String): List<AssistantSecret> = call("/assistants/$id/secrets") { client.get(it) { authorise() } }.body()

    suspend fun putAssistantSecret(id: String, name: String, request: PutAssistantSecretRequest): AssistantSecret =
        call("/assistants/$id/secrets/${name.encodeURLPathPart()}") { client.put(it) { authorise(); json(request) } }.body()

    suspend fun deleteAssistantSecret(id: String, name: String) {
        call("/assistants/$id/secrets/${name.encodeURLPathPart()}") { client.delete(it) { authorise() } }
    }

    private fun filePath(path: String): String = path.split('/').joinToString("/") { it.encodeURLPathPart() }

    suspend fun promptAssistant(id: String, text: String): Conversation =
        call("/assistants/$id/prompt") { client.post(it) { authorise(); json(PromptRequest(text)) } }.body()

    suspend fun clearAssistant(id: String): Assistant = call("/assistants/$id/clear") { client.post(it) { authorise() } }.body()

    suspend fun groups(): List<Group> = call("/groups") { client.get(it) { authorise() } }.body()

    suspend fun groupInstructions(id: String): InstructionLayer =
        call("/groups/$id/instructions") { client.get(it) { authorise() } }.body()

    suspend fun createGroup(name: String): Group =
        call("/groups") { client.post(it) { authorise(); json(GroupRequest(name)) } }.body()

    suspend fun renameGroup(id: String, name: String): Group =
        call("/groups/$id") { client.patch(it) { authorise(); json(GroupRequest(name)) } }.body()

    suspend fun saveGroupInstructions(id: String, text: String): InstructionLayer =
        call("/groups/$id/instructions") { client.put(it) { authorise(); json(SaveInstructionsRequest(text)) } }.body()

    suspend fun deleteGroup(id: String) {
        call("/groups/$id") { client.delete(it) { authorise() } }
    }

    suspend fun addGroupFolder(groupId: String, prefix: String, nodeId: String?): GroupFolder =
        call("/groups/$groupId/folders") { client.post(it) { authorise(); json(GroupFolderRequest(prefix, nodeId)) } }.body()

    suspend fun removeGroupFolder(groupId: String, folderId: String) {
        call("/groups/$groupId/folders/$folderId") { client.delete(it) { authorise() } }
    }

    suspend fun setProjectGroup(projectId: String, groupId: String?): Project =
        call("/projects/$projectId") { client.patch(it) { authorise(); json(PatchProjectRequest(groupId)) } }.body()

    suspend fun setProjectIcon(projectId: String, icon: String?): Project =
        call("/projects/$projectId") { client.patch(it) { authorise(); json(PatchProjectRequest(icon = icon, clearIcon = icon == null)) } }.body()

    suspend fun uploadProjectAvatar(projectId: String, mediaType: String, bytes: ByteArray): Project =
        call("/projects/$projectId/avatar") {
            client.post(it) {
                authorise()
                contentType(ContentType.parse(mediaType))
                setBody(bytes)
            }
        }.body()

    suspend fun extensions(): Extensions = call("/extensions") { client.get(it) { authorise() } }.body()

    suspend fun saveExtensions(extensions: Extensions): Extensions =
        call("/extensions") { client.put(it) { authorise(); json(extensions) } }.body()

    suspend fun experiments(): Experiments = call("/experiments") { client.get(it) { authorise() } }.body()

    suspend fun saveExperiments(experiments: Experiments): Experiments =
        call("/experiments") { client.put(it) { authorise(); json(experiments) } }.body()

    suspend fun usageLimitSetting(): UsageLimitSetting = call("/usage-limit") { client.get(it) { authorise() } }.body()

    suspend fun saveUsageLimitSetting(setting: UsageLimitSetting): UsageLimitSetting =
        call("/usage-limit") { client.put(it) { authorise(); json(setting) } }.body()

    suspend fun instructions(): Instructions = call("/instructions") { client.get(it) { authorise() } }.body()

    suspend fun saveInstructions(layer: String, text: String): InstructionLayer =
        call("/instructions/$layer") { client.put(it) { authorise(); json(SaveInstructionsRequest(text)) } }.body()

    suspend fun pullInstructions(): Instructions = call("/instructions/pull") { client.post(it) { authorise() } }.body()

    suspend fun instructionProposals(): List<InstructionProposal> = call("/instruction-proposals") { client.get(it) { authorise() } }.body()

    suspend fun approveInstructionProposal(id: String): InstructionProposal =
        call("/instruction-proposals/$id/approve") { client.post(it) { authorise() } }.body()

    suspend fun declineInstructionProposal(id: String, request: DeclineInstructionProposalRequest): InstructionProposal =
        call("/instruction-proposals/$id/decline") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun conversationConnections(): List<ConversationConnection> = call("/conversation-connections") { client.get(it) { authorise() } }.body()

    suspend fun connectConversations(request: CreateConversationConnectionRequest): ConversationConnection =
        call("/conversation-connections") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun revokeConversationConnection(id: String): ConversationConnection =
        call("/conversation-connections/$id") { client.delete(it) { authorise() } }.body()

    suspend fun approveAndConnectMessageProposal(id: String): MessageProposal =
        call("/message-proposals/$id/connect") { client.post(it) { authorise() } }.body()

    suspend fun messageProposals(): List<MessageProposal> = call("/message-proposals") { client.get(it) { authorise() } }.body()

    suspend fun approveMessageProposal(id: String): MessageProposal =
        call("/message-proposals/$id/approve") { client.post(it) { authorise() } }.body()

    suspend fun declineMessageProposal(id: String): MessageProposal =
        call("/message-proposals/$id/decline") { client.post(it) { authorise() } }.body()

    suspend fun apiAccessRequests(): List<ApiAccessRequest> = call("/api-access-requests") { client.get(it) { authorise() } }.body()

    suspend fun approveApiAccessRequest(id: String, scopes: List<String>): ApiAccessRequest =
        call("/api-access-requests/$id/approve") { client.post(it) { authorise(); json(ApproveApiAccessRequest(scopes)) } }.body()

    suspend fun declineApiAccessRequest(id: String): ApiAccessRequest =
        call("/api-access-requests/$id/decline") { client.post(it) { authorise() } }.body()

    suspend fun sshAccessRequests(): List<SshAccessRequest> = call("/ssh-access-requests") { client.get(it) { authorise() } }.body()

    suspend fun approveSshAccessRequest(id: String, minutes: Int): SshAccessRequest =
        call("/ssh-access-requests/$id/approve") { client.post(it) { authorise(); json(ApproveSshAccessRequest(minutes)) } }.body()

    suspend fun declineSshAccessRequest(id: String): SshAccessRequest =
        call("/ssh-access-requests/$id/decline") { client.post(it) { authorise() } }.body()

    suspend fun revokeSshAccessRequest(id: String): SshAccessRequest =
        call("/ssh-access-requests/$id/revoke") { client.post(it) { authorise() } }.body()

    suspend fun setInstructionSource(request: SetInstructionSourceRequest): Instructions =
        call("/instruction-source") { client.put(it) { authorise(); json(request) } }.body()

    suspend fun clearInstructionSource() {
        call("/instruction-source") { client.delete(it) { authorise() } }
    }

    suspend fun projects(): List<Project> = call("/projects") { client.get(it) { authorise() } }.body()

    suspend fun createProject(request: CreateProjectRequest): Project =
        call("/projects") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun worktrees(): List<Worktree> = call("/worktrees") { client.get(it) { authorise() } }.body()

    suspend fun tasks(): List<Task> = call("/tasks") { client.get(it) { authorise() } }.body()

    suspend fun createTask(request: CreateTaskRequest): Task =
        call("/tasks") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun startTask(id: String, conversationId: String? = null): Conversation =
        call("/tasks/$id/start") { client.post(it) { authorise(); conversationId?.let { target -> json(StartTaskRequest(target)) } } }.body()

    suspend fun approveTask(id: String): Task =
        call("/tasks/$id/approve") { client.post(it) { authorise() } }.body()

    suspend fun declineTask(id: String): Task =
        call("/tasks/$id/decline") { client.post(it) { authorise() } }.body()

    suspend fun answerTask(id: String, answer: String): Task =
        call("/tasks/$id/answer") { client.post(it) { authorise(); json(AnswerTaskRequest(answer)) } }.body()

    suspend fun finishTask(id: String): Task =
        call("/tasks/$id/done") { client.post(it) { authorise() } }.body()

    suspend fun deleteTask(id: String) {
        call("/tasks/$id") { client.delete(it) { authorise() } }
    }

    suspend fun artifacts(): List<Artifact> = call("/artifacts") { client.get(it) { authorise() } }.body()

    suspend fun artifactLinks(id: String): List<ArtifactLink> = call("/artifacts/$id/links") { client.get(it) { authorise() } }.body()

    suspend fun linkArtifact(id: String, request: LinkArtifactRequest): ArtifactLink =
        call("/artifacts/$id/links") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun unlinkArtifact(id: String, linkId: String) {
        call("/artifacts/$id/links/$linkId") { client.delete(it) { authorise() } }
    }

    suspend fun artifactAccessRequests(): List<ArtifactAccessRequest> = call("/artifact-access-requests") { client.get(it) { authorise() } }.body()

    suspend fun approveArtifactAccessRequest(id: String, project: Boolean): ArtifactAccessRequest =
        call("/artifact-access-requests/$id/approve") { client.post(it) { authorise(); json(ApproveArtifactAccessRequest(project)) } }.body()

    suspend fun declineArtifactAccessRequest(id: String): ArtifactAccessRequest =
        call("/artifact-access-requests/$id/decline") { client.post(it) { authorise() } }.body()

    suspend fun artifact(id: String): Artifact = call("/artifacts/$id") { client.get(it) { authorise() } }.body()

    suspend fun artifactThumbnail(id: String): ByteArray = call("/artifacts/$id/thumbnail") { client.get(it) { authorise() } }.body()

    suspend fun artifactContent(id: String, version: Int): ByteArray = call("/artifacts/$id/versions/$version/content") { client.get(it) { authorise() } }.body()

    suspend fun editArtifact(id: String, request: PatchArtifactRequest): Artifact =
        call("/artifacts/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun deleteArtifact(id: String) {
        call("/artifacts/$id") { client.delete(it) { authorise() } }
    }

    suspend fun schedules(): List<Schedule> = call("/schedules") { client.get(it) { authorise() } }.body()

    suspend fun createSchedule(request: CreateScheduleRequest): Schedule =
        call("/schedules") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun pauseSchedule(id: String, paused: Boolean): Schedule =
        call("/schedules/$id") { client.patch(it) { authorise(); json(PatchScheduleRequest(paused = paused)) } }.body()

    suspend fun editSchedule(id: String, request: PatchScheduleRequest): Schedule =
        call("/schedules/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun runSchedule(id: String): ScheduleRun =
        call("/schedules/$id/run") { client.post(it) { authorise() } }.body()

    suspend fun confirmSchedule(id: String, request: ConfirmScheduleRequest): Schedule =
        call("/schedules/$id/confirm") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun declineSchedule(id: String) {
        call("/schedules/$id/decline") { client.post(it) { authorise() } }
    }

    suspend fun deleteSchedule(id: String) {
        call("/schedules/$id") { client.delete(it) { authorise() } }
    }

    suspend fun setTimeZone(timeZone: String) {
        call("/me/time-zone") { client.put(it) { authorise(); json(SetTimeZoneRequest(timeZone)) } }
    }

    suspend fun scheduleRuns(): List<ScheduleRun> = call("/schedule-runs") { client.get(it) { authorise() } }.body()

    suspend fun issues(query: String = ""): Issues =
        call("/issues") { client.get(it) { authorise(); if (query.isNotBlank()) parameter("q", query.trim()) } }.body()

    suspend fun startWorkspace(projectId: String, number: Long, agent: String): Conversation =
        call("/projects/$projectId/issues/$number/workspace") { client.post(it) { authorise(); json(StartWorkspaceRequest(agent)) } }.body()

    suspend fun imports(): List<ImportStatus> = call("/imports") { client.get(it) { authorise() } }.body()

    suspend fun importJobs(): List<ImportJob> = call("/imports/jobs") { client.get(it) { authorise() } }.body()

    suspend fun startImport(request: ImportRequest): ImportJob =
        call("/imports") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun indexing(): Indexing = call("/indexing") { client.get(it) { authorise() } }.body()

    suspend fun search(query: String, limit: Int = 20): List<SearchHit> =
        call("/search") { client.get(it) { authorise(); parameter("q", query); parameter("limit", limit) } }.body()

    suspend fun conversations(archived: Boolean = false): List<Conversation> =
        call("/conversations") { client.get(it) { authorise(); parameter("archived", archived) } }.body()

    suspend fun createConversation(request: CreateConversationRequest): Conversation =
        call("/conversations") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun createChat(request: CreateChatRequest): Conversation =
        call("/chats") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun importClaudeChats(export: ByteArray): ImportResult =
        call("/chats/import") { client.post(it) { authorise(); contentType(ContentType.Application.Json); setBody(export) } }.body()

    suspend fun conversation(id: String): Conversation = call("/conversations/$id") { client.get(it) { authorise() } }.body()

    suspend fun patchConversation(id: String, request: PatchConversationRequest): Conversation =
        call("/conversations/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun continueConversation(id: String, request: ContinueRequest): Conversation =
        call("/conversations/$id/continue") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun retryConversation(id: String) {
        call("/conversations/$id/retry") { client.post(it) { authorise() } }
    }

    suspend fun scheduleResume(id: String) {
        call("/conversations/$id/resume") { client.post(it) { authorise() } }
    }

    suspend fun cancelResume(id: String) {
        call("/conversations/$id/resume") { client.delete(it) { authorise() } }
    }

    suspend fun deleteConversation(id: String) {
        call("/conversations/$id") { client.delete(it) { authorise() } }
    }

    suspend fun startSideChat(id: String, kind: String, text: String?): Conversation =
        call("/conversations/$id/side") { client.post(it) { authorise(); json(StartSideChatRequest(kind, text)) } }.body()

    suspend fun movePlan(id: String, nodeId: String): MovePlan = call("/conversations/$id/move-plan") { client.get(it) { authorise(); parameter("node_id", nodeId) } }.body()

    suspend fun requestMove(id: String, request: MoveRequest): MoveProgress = call("/conversations/$id/move") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun currentMove(id: String): MoveProgress? = call("/conversations/$id/move") { client.get(it) { authorise() } }.body()

    suspend fun cancelMove(id: String) {
        call("/conversations/$id/move") { client.delete(it) { authorise() } }
    }

    suspend fun goal(id: String): Goal? = call("/conversations/$id/goal") { client.get(it) { authorise() } }.body()

    suspend fun setGoal(id: String, request: SetGoalRequest): Goal = call("/conversations/$id/goal") { client.put(it) { authorise(); json(request) } }.body()

    suspend fun pauseGoal(id: String, paused: Boolean): Goal = call("/conversations/$id/goal") { client.patch(it) { authorise(); json(PatchGoalRequest(paused)) } }.body()

    suspend fun clearGoal(id: String) {
        call("/conversations/$id/goal") { client.delete(it) { authorise() } }
    }

    suspend fun walkthrough(id: String): Walkthrough? = call("/conversations/$id/walkthrough") { client.get(it) { authorise() } }.body()

    suspend fun walkthroughUncovered(id: String): List<WalkthroughFile> = call("/walkthroughs/$id/uncovered") { client.get(it) { authorise() } }.body()

    suspend fun walkthroughReads(id: String): List<Int> = call("/walkthroughs/$id/reads") { client.get(it) { authorise() } }.body()

    suspend fun markWalkthroughRead(id: String, step: Int) {
        call("/walkthroughs/$id/reads/$step") { client.put(it) { authorise() } }
    }

    suspend fun addWalkthroughNote(id: String, request: WalkthroughNoteRequest): WalkthroughNote = call("/walkthroughs/$id/notes") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun walkthroughNotes(id: String): List<WalkthroughNote> = call("/walkthroughs/$id/notes") { client.get(it) { authorise() } }.body()

    suspend fun markWalkthroughNotesSent(id: String) {
        call("/walkthroughs/$id/notes/sent") { client.post(it) { authorise() } }
    }

    suspend fun removeWalkthroughNote(id: String, noteId: String) {
        call("/walkthroughs/$id/notes/$noteId") { client.delete(it) { authorise() } }
    }

    suspend fun dismissWalkthrough(id: String) {
        call("/walkthroughs/$id/dismissal") { client.put(it) { authorise() } }
    }

    suspend fun reopenWalkthrough(id: String) {
        call("/walkthroughs/$id/dismissal") { client.delete(it) { authorise() } }
    }

    suspend fun markOpened(id: String) {
        call("/conversations/$id/opened") { client.post(it) { authorise() } }
    }

    suspend fun files(id: String, query: String): List<String> =
        call("/conversations/$id/files") { client.get(it) { authorise(); parameter("query", query) } }.body()

    suspend fun gitStatus(id: String): GitStatus = call("/conversations/$id/git/status") { client.get(it) { authorise() } }.body()

    suspend fun gitLog(id: String, limit: Int = 50): List<GitCommit> =
        call("/conversations/$id/git/log") { client.get(it) { authorise(); parameter("limit", limit) } }.body()

    suspend fun gitGraph(id: String, skip: Int = 0, limit: Int = 200): List<GraphCommit> =
        call("/conversations/$id/git/graph") { client.get(it) { authorise(); parameter("skip", skip); parameter("limit", limit) } }.body()

    suspend fun gitShow(id: String, hash: String): GitDiff =
        call("/conversations/$id/git/show") { client.get(it) { authorise(); parameter("hash", hash) } }.body()

    suspend fun gitBranches(id: String): GitBranches = call("/conversations/$id/git/branches") { client.get(it) { authorise() } }.body()

    suspend fun gitStack(id: String): GitStack = call("/conversations/$id/git/stack") { client.get(it) { authorise() } }.body()

    suspend fun gitSwitch(id: String, branch: String, create: Boolean = false, track: String? = null) {
        call("/conversations/$id/git/switch") { client.post(it) { authorise(); json(GitSwitchRequest(branch, create, track)) } }
    }

    suspend fun gitDiff(id: String, path: String, staged: Boolean): GitDiff =
        call("/conversations/$id/git/diff") { client.get(it) { authorise(); parameter("path", path); parameter("staged", staged) } }.body()

    suspend fun gitChangesLayout(id: String): ChangesLayout =
        call("/conversations/$id/git/changes-layout") { client.get(it) { authorise() } }.body()

    suspend fun gitChangesRows(id: String, ranges: List<RowRange>): ChangesRows =
        call("/conversations/$id/git/changes-rows") { client.post(it) { authorise(); json(RowsRequest(ranges)) } }.body()

    suspend fun gitStage(id: String, paths: List<String>) {
        call("/conversations/$id/git/stage") { client.post(it) { authorise(); json(GitPathsRequest(paths)) } }
    }

    suspend fun gitUnstage(id: String, paths: List<String>) {
        call("/conversations/$id/git/unstage") { client.post(it) { authorise(); json(GitPathsRequest(paths)) } }
    }

    suspend fun gitHunk(id: String, path: String, row: Int, fingerprint: String, action: String) {
        call("/conversations/$id/git/hunk") { client.post(it) { authorise(); json(GitHunkRequest(path, row, fingerprint, action)) } }
    }

    suspend fun gitFile(id: String, path: String, content: String? = null, expected: String? = null): GitFileContent =
        call("/conversations/$id/git/file") { client.post(it) { authorise(); json(GitFileRequest(path, content, expected)) } }.body()

    suspend fun gitStageAll(id: String) {
        call("/conversations/$id/git/stage-all") { client.post(it) { authorise() } }
    }

    suspend fun gitCommit(id: String, message: String, amend: Boolean = false, signoff: Boolean = false, noVerify: Boolean = false) {
        call("/conversations/$id/git/commit") { client.post(it) { authorise(); json(GitCommitRequest(message, amend, signoff, noVerify)) } }
    }

    suspend fun gitFetch(id: String) {
        call("/conversations/$id/git/fetch") { client.post(it) { authorise() } }
    }

    suspend fun gitPull(id: String, rebase: Boolean = false) {
        call("/conversations/$id/git/pull") { client.post(it) { authorise(); json(GitPullRequest(rebase)) } }
    }

    suspend fun gitPush(id: String, force: Boolean = false) {
        call("/conversations/$id/git/push") { client.post(it) { authorise(); json(GitPushRequest(force)) } }
    }

    suspend fun gitUndo(id: String): GitUndone = call("/conversations/$id/git/undo") { client.post(it) { authorise() } }.body()

    suspend fun gitTownSync(id: String) {
        call("/conversations/$id/git/town/sync") { client.post(it) { authorise() } }
    }

    suspend fun gitTownPropose(id: String) {
        call("/conversations/$id/git/town/propose") { client.post(it) { authorise() } }
    }

    suspend fun sessionUpdates(id: String): List<JsonObject> = call("/conversations/$id/session") { client.get(it) { authorise() } }.body()

    suspend fun events(id: String, after: Long = 0, limit: Int = 500, before: Long? = null): List<Event> =
        call("/conversations/$id/events") {
            client.get(it) {
                authorise()

                if (before == null) {
                    parameter("after", after)
                } else {
                    parameter("before", before)
                }

                parameter("limit", limit)
            }
        }.body()

    suspend fun sendPrompt(id: String, text: String, attachments: List<Attachment> = emptyList()) {
        call("/conversations/$id/prompt") { client.post(it) { authorise(); json(PromptRequest(text, attachments)) } }
    }

    suspend fun uploadAttachment(id: String, name: String, mediaType: String, bytes: ByteArray): Attachment =
        call("/conversations/$id/attachments") {
            client.post(it) {
                authorise()
                parameter("name", name)
                contentType(ContentType.parse(mediaType))
                setBody(bytes)
            }
        }.body()

    suspend fun attachmentBytes(sha256: String): ByteArray =
        call("/attachments/$sha256") { client.get(it) { authorise() } }.body()

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

    suspend fun sendAllQueuedNow(id: String) {
        call("/conversations/$id/queue/send-all-now") { client.post(it) { authorise() } }
    }

    suspend fun cancel(id: String) {
        call("/conversations/$id/cancel") { client.post(it) { authorise() } }
    }

    suspend fun closeSession(id: String) {
        call("/conversations/$id/close") { client.post(it) { authorise() } }
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
