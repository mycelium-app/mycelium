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
import io.ktor.client.statement.bodyAsText
import io.ktor.http.ContentType
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.contentType
import io.ktor.http.isSuccess
import io.ktor.serialization.kotlinx.json.json
import io.ktor.websocket.Frame
import io.ktor.websocket.readText
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive

private const val OlderUnknownRouteMessage = "That address was not found. Check the address and try again."
private const val UnknownRouteMessage = "This control plane does not offer that yet. Update it to the latest version of Mycelium, then try again."

const val ControlPlaneUnreachable = "Could not reach the control plane. Check your connection and try again."

open class ApiException(val status: Int, val code: String, override val message: String) : Exception(message)

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

    suspend fun workflows(): List<Workflow> = call("/workflows") { client.get(it) { authorise() } }.body()

    suspend fun workflow(id: String): Workflow = call("/workflows/$id") { client.get(it) { authorise() } }.body()

    suspend fun createWorkflow(plan: WorkflowPlan): Workflow = call("/workflows") { client.post(it) { authorise(); json(plan) } }.body()

    suspend fun rerunWorkflow(id: String): Workflow = call("/workflows/$id/rerun") { client.post(it) { authorise() } }.body()

    suspend fun controlWorkflow(id: String, request: WorkflowControlRequest): Workflow = call("/workflows/$id/control") { client.post(it) { authorise(); json(request) } }.body()

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

    suspend fun usageHistory(range: String, account: String? = null): UsageHistory =
        call("/usage/history") { client.get(it) { authorise(); parameter("range", range); if (account != null) parameter("account", account) } }.body()

    suspend fun usageOf(id: String): ConversationUsage = call("/conversations/$id/usage") { client.get(it) { authorise() } }.body()

    suspend fun conversationUsage(): List<ActivityRow> = call("/activity/conversations") { client.get(it) { authorise() } }.body()

    suspend fun nodes(): List<NodeSummary> = call("/nodes") { client.get(it) { authorise() } }.body()

    suspend fun mintSshToken(nodeId: String): SshToken = call("/nodes/$nodeId/ssh-tokens") { client.post(it) { authorise() } }.body()

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

    suspend fun startEnvironment(conversationId: String, request: StartEnvironmentRequest = StartEnvironmentRequest()): Environment =
        call("/conversations/$conversationId/environment") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun lastEnvironmentNode(): LastEnvironmentNode =
        call("/environment/last-node") { client.get(it) { authorise() } }.body()

    suspend fun openEnvironmentApp(conversationId: String, app: String): Environment =
        call("/conversations/$conversationId/environment/apps") { client.post(it) { authorise(); json(OpenEnvironmentAppRequest(app)) } }.body()

    suspend fun focusEnvironmentWindow(conversationId: String, window: Long): Environment =
        call("/conversations/$conversationId/environment/windows/$window/focus") { client.post(it) { authorise() } }.body()

    suspend fun environmentStreamTicket(conversationId: String): String =
        call("/conversations/$conversationId/environment/stream-ticket") { client.post(it) { authorise() } }.body<JsonObject>()["ticket"]!!.jsonPrimitive.content

    suspend fun environment(conversationId: String): Environment? =
        try {
            call("/conversations/$conversationId/environment") { client.get(it) { authorise() } }.body<Environment>()
        } catch (gone: ApiException) {
            if (gone.status == 404) null else throw gone
        }

    suspend fun stopEnvironment(conversationId: String) {
        call("/conversations/$conversationId/environment") { client.delete(it) { authorise() } }
    }

    suspend fun takeOverEnvironment(conversationId: String): Environment =
        call("/conversations/$conversationId/environment/take-over") { client.post(it) { authorise() } }.body()

    suspend fun handBackEnvironment(conversationId: String): Environment =
        call("/conversations/$conversationId/environment/hand-back") { client.post(it) { authorise() } }.body()

    suspend fun stopBrowserTask(conversationId: String): Environment =
        call("/conversations/$conversationId/environment/browser-task/stop") { client.post(it) { authorise() } }.body()

    suspend fun environmentActions(conversationId: String): List<EnvironmentAction> =
        call("/conversations/$conversationId/environment/actions") { client.get(it) { authorise() } }.body()

    suspend fun checkEnvironment(nodeId: String): EnvironmentCapability? {
        val answer = call("/nodes/$nodeId/environment/check") { client.post(it) { authorise() } }.bodyAsText()

        return if (answer.isBlank()) null else MyceliumJson.decodeFromString(EnvironmentCapability.serializer(), answer)
    }

    suspend fun memories(query: String = ""): List<Memory> =
        call("/memories") { client.get(it) { authorise(); if (query.isNotBlank()) parameter("query", query.trim()) } }.body()

    suspend fun updateMemory(id: String, text: String): Memory =
        call("/memories/$id") { client.patch(it) { authorise(); json(UpdateMemoryRequest(text)) } }.body()

    suspend fun deleteMemory(id: String) {
        call("/memories/$id") { client.delete(it) { authorise() } }
    }

    suspend fun memoryBanks(): List<MemoryBank> = call("/memory-banks") { client.get(it) { authorise() } }.body()

    suspend fun createMemoryBank(request: SaveMemoryBankRequest): MemoryBank =
        call("/memory-banks") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun updateMemoryBank(id: String, request: SaveMemoryBankRequest): MemoryBank =
        call("/memory-banks/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun deleteMemoryBank(id: String) {
        call("/memory-banks/$id") { client.delete(it) { authorise() } }
    }

    suspend fun moveMemory(id: String, bankId: String?): Memory =
        call("/memories/$id/bank") { client.put(it) { authorise(); json(MoveMemoryRequest(bankId)) } }.body()

    suspend fun forges(): List<Forge> = call("/forges") { client.get(it) { authorise() } }.body()

    suspend fun connectForge(request: ConnectForgeRequest): Forge =
        call("/forges") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun linkForgeMcp(id: String): Forge = call("/forges/$id/mcp") { client.post(it) { authorise() } }.body()

    suspend fun disconnectForge(id: String) {
        call("/forges/$id") { client.delete(it) { authorise() } }
    }

    suspend fun modelProviders(): List<ModelProvider> = call("/model-providers") { client.get(it) { authorise() } }.body()

    suspend fun decisionProviders(): List<DecisionProvider> = call("/decision-providers") { client.get(it) { authorise() } }.body()

    suspend fun createDecisionProvider(request: CreateDecisionProviderRequest): DecisionProvider =
        call("/decision-providers") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun updateDecisionProvider(id: String, request: PatchDecisionProviderRequest): DecisionProvider =
        call("/decision-providers/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun deleteDecisionProvider(id: String) {
        call("/decision-providers/$id") { client.delete(it) { authorise() } }
    }

    suspend fun testDecisionProvider(request: TestDecisionProviderRequest): DecisionProviderModels =
        call("/decision-providers/test") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun modelProviderUsage(): List<ModelProviderUsage> = call("/model-provider-usage") { client.get(it) { authorise() } }.body()

    suspend fun createModelProvider(request: CreateModelProviderRequest): ModelProvider =
        call("/model-providers") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun updateModelProvider(id: String, request: UpdateModelProviderRequest): ModelProvider =
        call("/model-providers/$id") { client.patch(it) { authorise(); json(request) } }.body()

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

    suspend fun startLastFmSignIn(id: String): OAuthStart =
        call("/chat-tool-servers/$id/lastfm-sign-in") { client.post(it) { authorise() } }.body()

    suspend fun signOutOfLastFm(id: String) {
        call("/chat-tool-servers/$id/lastfm-sign-in") { client.delete(it) { authorise() } }
    }

    suspend fun startFreeAgentSignIn(id: String): OAuthStart =
        call("/chat-tool-servers/$id/freeagent-sign-in") { client.post(it) { authorise() } }.body()

    suspend fun signOutOfFreeAgent(id: String) {
        call("/chat-tool-servers/$id/freeagent-sign-in") { client.delete(it) { authorise() } }
    }

    suspend fun patchChatToolServer(id: String, request: PatchChatToolServerRequest): ChatToolServer =
        call("/chat-tool-servers/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun refreshChatToolServer(id: String): ChatToolServer =
        call("/chat-tool-servers/$id/refresh") { client.post(it) { authorise() } }.body()

    suspend fun setChatToolServerGroups(id: String, groups: List<String>): ChatToolServer =
        call("/chat-tool-servers/$id/groups") { client.put(it) { authorise(); json(SetIntegrationGroupsRequest(groups)) } }.body()

    suspend fun integrationCalls(server: String? = null, before: String? = null): List<IntegrationCall> =
        call("/integration-calls") { client.get(it) { authorise(); server?.let { value -> parameter("server", value) }; before?.let { value -> parameter("before", value) } } }.body()

    suspend fun probeChatToolServer(request: ProbeChatToolServerRequest): ChatToolServerProbe =
        call("/chat-tool-servers/probe") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun testChatToolServer(request: TestChatToolServerRequest): TestedChatToolServer =
        call("/chat-tool-servers/test") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun testCalendars(request: TestCalendarsRequest): TestedCalendars =
        call("/chat-tool-servers/caldav-test") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun testCoolify(request: AppsCredentialRequest): CoolifyTested =
        call("/chat-tool-servers/coolify-test") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun testTandoor(request: AppsCredentialRequest): TandoorTested =
        call("/chat-tool-servers/tandoor-test") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun coolifyOptions(id: String): CoolifyOptions = call("/chat-tool-servers/$id/coolify-options") { client.get(it) { authorise() } }.body()

    suspend fun setCoolifyTarget(id: String, request: CoolifyTargetRequest): ChatToolServer =
        call("/chat-tool-servers/$id/coolify-target") { client.put(it) { authorise(); json(request) } }.body()

    suspend fun integrationCalendars(id: String): List<CalendarAccess> = call("/integrations/$id/calendars") { client.get(it) { authorise() } }.body()

    suspend fun integrationApiGrants(id: String): List<IntegrationApiGrant> = call("/chat-tool-servers/$id/api-access") { client.get(it) { authorise() } }.body()

    suspend fun deleteChatToolServer(id: String) {
        call("/chat-tool-servers/$id") { client.delete(it) { authorise() } }
    }

    suspend fun apps(): List<AppSummary> = call("/apps") { client.get(it) { authorise() } }.body<AppList>().apps

    suspend fun app(uuid: String): AppDetail = call("/apps/$uuid") { client.get(it) { authorise() } }.body()

    suspend fun appDeployments(uuid: String): List<AppDeployment> = call("/apps/$uuid/deployments") { client.get(it) { authorise() } }.body()

    suspend fun appDeployment(uuid: String): DeploymentProgress = call("/app-deployments/$uuid") { client.get(it) { authorise() } }.body()

    suspend fun appLogs(uuid: String, kind: String, deployment: String? = null, lines: Int = 200): AppLogs =
        call("/apps/$uuid/logs") {
            client.get(it) {
                authorise()
                parameter("kind", kind)
                parameter("lines", lines)
                deployment?.let { value -> parameter("deployment", value) }
            }
        }.body()

    suspend fun appEnvs(uuid: String): List<AppEnv> = call("/apps/$uuid/envs") { client.get(it) { authorise() } }.body()

    suspend fun putAppEnv(uuid: String, key: String, request: PutAppEnvRequest): AppEnv =
        call("/apps/$uuid/envs/${key.encodeURLPathPart()}") { client.put(it) { authorise(); json(request) } }.body()

    suspend fun deleteAppEnv(uuid: String, key: String) {
        call("/apps/$uuid/envs/${key.encodeURLPathPart()}") { client.delete(it) { authorise() } }
    }

    suspend fun redeployApp(uuid: String): AppRedeployed = call("/apps/$uuid/redeploy") { client.post(it) { authorise() } }.body()

    suspend fun startApp(uuid: String) {
        call("/apps/$uuid/start") { client.post(it) { authorise() } }
    }

    suspend fun stopApp(uuid: String) {
        call("/apps/$uuid/stop") { client.post(it) { authorise() } }
    }

    suspend fun deleteApp(uuid: String) {
        call("/apps/$uuid") { client.delete(it) { authorise() } }
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

    suspend fun promptAssistant(id: String, text: String, spoken: Boolean = false): Conversation =
        call("/assistants/$id/prompt") { client.post(it) { authorise(); json(PromptRequest(text, spoken = spoken)) } }.body()

    suspend fun voiceAssistant(): Assistant = call("/assistants/builtin/voice") { client.get(it) { authorise() } }.body()

    suspend fun wellnessAssistant(): Assistant = call("/assistants/builtin/wellness") { client.get(it) { authorise() } }.body()

    suspend fun resetAssistantInstructions(id: String): Assistant = call("/assistants/$id/instructions/reset") { client.post(it) { authorise() } }.body()

    suspend fun assistantActions(id: String): List<AssistantAction> = call("/assistants/$id/actions") { client.get(it) { authorise() } }.body()

    suspend fun createAssistantAction(id: String, request: SaveActionRequest): AssistantAction =
        call("/assistants/$id/actions") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun saveAssistantAction(id: String, actionId: String, request: SaveActionRequest): AssistantAction =
        call("/assistants/$id/actions/$actionId") { client.put(it) { authorise(); json(request) } }.body()

    suspend fun deleteAssistantAction(id: String, actionId: String) {
        call("/assistants/$id/actions/$actionId") { client.delete(it) { authorise() } }
    }

    suspend fun resetAssistantAction(id: String, actionId: String): AssistantAction =
        call("/assistants/$id/actions/$actionId/reset") { client.post(it) { authorise() } }.body()

    suspend fun runAssistantAction(id: String, actionId: String, inputs: Map<String, String>): Conversation =
        call("/assistants/$id/actions/$actionId/run") { client.post(it) { authorise(); json(RunActionRequest(inputs)) } }.body()

    suspend fun actionProposals(id: String): List<ActionProposal> = call("/assistants/$id/action-proposals") { client.get(it) { authorise() } }.body()

    suspend fun approveActionProposal(id: String, proposalId: String, edited: SaveActionRequest? = null): AssistantAction =
        call("/assistants/$id/action-proposals/$proposalId/approve") { client.post(it) { authorise(); edited?.let { body -> json(body) } } }.body()

    suspend fun declineActionProposal(id: String, proposalId: String): ActionProposal =
        call("/assistants/$id/action-proposals/$proposalId/decline") { client.post(it) { authorise() } }.body()

    suspend fun assistantChanges(): List<AssistantChange> = call("/assistant-changes") { client.get(it) { authorise() } }.body()

    suspend fun approveAssistantChange(id: String): AssistantChange = call("/assistant-changes/$id/approve") { client.post(it) { authorise() } }.body()

    suspend fun declineAssistantChange(id: String): AssistantChange = call("/assistant-changes/$id/decline") { client.post(it) { authorise() } }.body()

    suspend fun undoAssistantChange(id: String): AssistantChange = call("/assistant-changes/$id/undo") { client.post(it) { authorise() } }.body()

    suspend fun clearAssistant(id: String): Assistant = call("/assistants/$id/clear") { client.post(it) { authorise() } }.body()

    suspend fun groups(): List<Group> = call("/groups") { client.get(it) { authorise() } }.body()

    suspend fun groupInstructions(id: String): InstructionLayer =
        call("/groups/$id/instructions") { client.get(it) { authorise() } }.body()

    suspend fun createGroup(name: String): Group =
        call("/groups") { client.post(it) { authorise(); json(GroupRequest(name)) } }.body()

    suspend fun renameGroup(id: String, name: String): Group =
        call("/groups/$id") { client.patch(it) { authorise(); json(GroupRequest(name)) } }.body()

    suspend fun saveGroupHours(id: String, hours: Hours): Group =
        call("/groups/$id/hours") { client.put(it) { authorise(); json(hours) } }.body()

    suspend fun openGroup(id: String, reason: String): GroupOpening =
        call("/groups/$id/open") { client.post(it) { authorise(); json(OpenGroupRequest(reason)) } }.body()

    suspend fun closeGroupOpening(id: String) {
        call("/groups/$id/open") { client.delete(it) { authorise() } }
    }

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

    suspend fun agentSettings(): AgentSettings = call("/agent-settings") { client.get(it) { authorise() } }.body()

    suspend fun saveAgentSettings(request: SaveAgentSettingsRequest): AgentSettings =
        call("/agent-settings") { client.put(it) { authorise(); json(request) } }.body()

    suspend fun setAccountMode(request: SetAccountModeRequest) {
        call("/agent-settings/account-mode") { client.put(it) { authorise(); json(request) } }
    }

    suspend fun accountPrices(): List<AccountPrice> = call("/account-prices") { client.get(it) { authorise() } }.body()

    suspend fun setAccountPrice(request: SetAccountPriceRequest) {
        call("/account-prices") { client.put(it) { authorise(); json(request) } }
    }

    suspend fun wellnessSettings(): WellnessSettings = call("/wellness/settings") { client.get(it) { authorise() } }.body()

    suspend fun saveWellnessSettings(settings: WellnessSettings): WellnessSettings =
        call("/wellness/settings") { client.put(it) { authorise(); json(settings) } }.body()

    suspend fun wellnessOverview(days: Int = 7): WellnessOverview = call("/wellness/overview?days=$days") { client.get(it) { authorise() } }.body()

    suspend fun activityDay(date: String? = null): ActivityDay = call("/wellness/activity/day" + (date?.let { "?date=$it" } ?: "")) { client.get(it) { authorise() } }.body()

    suspend fun activityHours(days: Int = 7): ActivityHours = call("/wellness/activity/hours?days=$days") { client.get(it) { authorise() } }.body()

    suspend fun wellnessHeartbeat(beat: WellnessHeartbeat) {
        call("/wellness/heartbeat") { client.post(it) { authorise(); json(beat) } }
    }

    suspend fun startWellnessBreak(minutes: Int): WellnessBreak =
        call("/wellness/breaks") { client.post(it) { authorise(); json(StartBreakRequest(minutes)) } }.body()

    suspend fun endWellnessBreak() {
        call("/wellness/breaks/current") { client.delete(it) { authorise() } }
    }

    suspend fun saveBreakNote(id: String, note: String): WellnessBreak =
        call("/wellness/breaks/$id/note") { client.put(it) { authorise(); json(BreakNoteRequest(note)) } }.body()

    suspend fun recordPuzzleResult(puzzle: String, result: PuzzleResult) {
        call("/wellness/puzzles/$puzzle/today") { client.put(it) { authorise(); json(result) } }
    }

    suspend fun learned(limit: Int = 5): List<LearnedItem> = call("/wellness/learned?limit=$limit") { client.get(it) { authorise() } }.body()

    suspend fun puzzles(days: Int = 366): List<PuzzleSummary> = call("/wellness/puzzles?days=$days") { client.get(it) { authorise() } }.body()

    suspend fun wordleGame(): WordleGame = call("/wellness/puzzles/wordle/game") { client.get(it) { authorise() } }.body()

    suspend fun guessWordle(word: String): WordleGame = call("/wellness/puzzles/wordle/guesses") { client.post(it) { authorise(); json(WordleGuessRequest(word)) } }.body()

    suspend fun connectionsGame(): ConnectionsGame = call("/wellness/puzzles/connections/game") { client.get(it) { authorise() } }.body()

    suspend fun guessConnections(words: List<String>): ConnectionsGuessAnswer = call("/wellness/puzzles/connections/guesses") { client.post(it) { authorise(); json(ConnectionsGuessRequest(words)) } }.body()

    suspend fun holdQuietly(kind: String, subject: String, groupId: String?) {
        call("/wellness/holds") { client.post(it) { authorise(); json(HoldRequest(kind, subject, groupId)) } }
    }

    suspend fun reflections(): List<Reflection> = call("/reflections") { client.get(it) { authorise() } }.body()

    suspend fun patchReflection(id: String, request: PatchReflectionRequest): Reflection =
        call("/reflections/$id") { client.patch(it) { authorise(); json(request) } }.body()

    suspend fun redraftReflection(id: String): RedraftReflection = call("/reflections/$id/redraft") { client.post(it) { authorise() } }.body()


    suspend fun skills(hidden: Boolean = false): List<Skill> = call("/skills?hidden=$hidden") { client.get(it) { authorise() } }.body()

    suspend fun hideSkill(skill: Skill) {
        call("/skills/hidden") { client.put(it) { authorise(); json(HiddenSkillRequest(skill.kind, skill.name)) } }
    }

    suspend fun unhideSkill(skill: Skill) {
        call("/skills/hidden") { client.delete(it) { authorise(); json(HiddenSkillRequest(skill.kind, skill.name)) } }
    }

    suspend fun exercises(): List<Exercise> = call("/exercises") { client.get(it) { authorise() } }.body()

    suspend fun suggestedExercises(): List<Exercise> = call("/exercises/suggested") { client.get(it) { authorise() } }.body()

    suspend fun startExercise(id: String): Exercise = call("/exercises/$id/start") { client.post(it) { authorise() } }.body()

    suspend fun exerciseNotNow(id: String): Exercise = call("/exercises/$id/not-now") { client.post(it) { authorise() } }.body()

    suspend fun runExerciseTests(id: String): ExerciseTests = call("/exercises/$id/tests") { client.post(it) { authorise() } }.body()

    suspend fun reviewExercise(id: String): Exercise = call("/exercises/$id/review") { client.post(it) { authorise() } }.body()

    suspend fun finishExercise(id: String): Exercise = call("/exercises/$id/done") { client.post(it) { authorise() } }.body()

    suspend fun exerciseSolution(id: String): List<SolutionFile> = call("/exercises/$id/solution") { client.get(it) { authorise() } }.body()

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

    suspend fun revokeConversationConnection(id: String): ConversationConnection =
        call("/conversation-connections/$id") { client.delete(it) { authorise() } }.body()

    suspend fun groupConnections(): List<GroupConnection> = call("/group-connections") { client.get(it) { authorise() } }.body()

    suspend fun connectGroups(request: CreateGroupConnectionRequest): GroupConnection =
        call("/group-connections") { client.post(it) { authorise(); json(request) } }.body()

    suspend fun revokeGroupConnection(id: String): GroupConnection =
        call("/group-connections/$id") { client.delete(it) { authorise() } }.body()

    suspend fun approveAndConnectMessageProposal(id: String): MessageProposal =
        call("/message-proposals/$id/connect") { client.post(it) { authorise() } }.body()

    suspend fun approveAndConnectGroups(id: String): MessageProposal =
        call("/message-proposals/$id/connect-groups") { client.post(it) { authorise() } }.body()

    suspend fun messageProposals(): List<MessageProposal> = call("/message-proposals") { client.get(it) { authorise() } }.body()

    suspend fun approveMessageProposal(id: String): MessageProposal =
        call("/message-proposals/$id/approve") { client.post(it) { authorise() } }.body()

    suspend fun declineMessageProposal(id: String): MessageProposal =
        call("/message-proposals/$id/decline") { client.post(it) { authorise() } }.body()

    suspend fun startProposals(): List<StartProposal> = call("/start-proposals") { client.get(it) { authorise() } }.body()

    suspend fun approveStartProposal(id: String, choices: Map<String, StartProposalChoice>): StartProposal =
        call("/start-proposals/$id/approve") { client.post(it) { authorise(); json(ApproveStartProposalRequest(choices)) } }.body()

    suspend fun approveAndConnectStartProposal(id: String, choices: Map<String, StartProposalChoice>): StartProposal =
        call("/start-proposals/$id/connect") { client.post(it) { authorise(); json(ApproveStartProposalRequest(choices)) } }.body()

    suspend fun declineStartProposal(id: String): StartProposal =
        call("/start-proposals/$id/decline") { client.post(it) { authorise() } }.body()

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

    suspend fun createArtifact(request: CreateArtifactRequest): Artifact =
        call("/artifacts") { client.post(it) { authorise(); json(request) } }.body()

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

    suspend fun dismissConversation(id: String) {
        call("/conversations/$id/dismiss") { client.post(it) { authorise() } }
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

    suspend fun sendGoalAgain(id: String): Goal = call("/conversations/$id/goal/send") { client.post(it) { authorise() } }.body()

    suspend fun nudgeGoal(id: String): Goal = call("/conversations/$id/goal/nudge") { client.post(it) { authorise() } }.body()

    suspend fun continueGoal(id: String, note: String?): Goal = call("/conversations/$id/goal/continue") { client.post(it) { authorise(); json(ContinueGoalRequest(note)) } }.body()

    suspend fun clearGoal(id: String) {
        call("/conversations/$id/goal") { client.delete(it) { authorise() } }
    }

    suspend fun walkthrough(id: String): Walkthrough? = call("/conversations/$id/walkthrough") { client.get(it) { authorise() } }.body()

    suspend fun walkthroughUncovered(id: String): List<WalkthroughFile> = call("/walkthroughs/$id/uncovered") { client.get(it) { authorise() } }.body()

    suspend fun walkthroughReads(id: String): List<Int> = call("/walkthroughs/$id/reads") { client.get(it) { authorise() } }.body()

    suspend fun walkthroughAnswers(id: String): List<WalkthroughAnswer> = call("/walkthroughs/$id/answers") { client.get(it) { authorise() } }.body()

    suspend fun answerWalkthroughCheck(id: String, step: Int, request: WalkthroughAnswerRequest): WalkthroughAnswer =
        call("/walkthroughs/$id/answers/$step") { client.put(it) { authorise(); json(request) } }.body()

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

    suspend fun sendPrompt(id: String, text: String, attachments: List<Attachment> = emptyList()): String? {
        val response = call("/conversations/$id/prompt") { client.post(it) { authorise(); json(PromptRequest(text, attachments)) } }

        if (response.status != HttpStatusCode.Accepted) {
            return null
        }

        return response.body<PromptAccepted>().message
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

    suspend fun transcribe(audio: ByteArray): String =
        call("/transcriptions") {
            client.post(it) {
                authorise()
                contentType(ContentType("audio", "wav"))
                setBody(audio)
            }
        }.body<Transcription>().text

    suspend fun privacy(): PrivacySettings = call("/privacy") { client.get(it) { authorise() } }.body()

    suspend fun savePrivacy(request: SavePrivacyRequest): PrivacySettings = call("/privacy") { client.put(it) { authorise(); json(request) } }.body()

    suspend fun speech(request: SpeechRequest): SpokenAudio {
        val response = call("/speech") { client.post(it) { authorise(); json(request) } }

        return SpokenAudio(response.body(), response.contentType()?.let { "${it.contentType}/${it.contentSubtype}" } ?: "audio/mpeg")
    }

    suspend fun speechVoices(): List<String> = call("/speech/voices") { client.get(it) { authorise() } }.body<SpeechVoices>().voices

    suspend fun attachmentBytes(sha256: String): ByteArray =
        call("/attachments/$sha256") { client.get(it) { authorise() } }.body()

    suspend fun queue(id: String): List<QueuedPrompt> = call("/conversations/$id/queue") { client.get(it) { authorise() } }.body()

    suspend fun editQueued(id: String, queueId: String, request: PatchQueueRequest) {
        call("/conversations/$id/queue/$queueId") { client.patch(it) { authorise(); json(request) } }
    }

    suspend fun moveQueued(id: String, queueId: String, index: Int) {
        call("/conversations/$id/queue/$queueId/move") { client.post(it) { authorise(); json(MoveQueuedRequest(index)) } }
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

    suspend fun response(id: String, requestId: String): ResponseToEdit? =
        try {
            call("/conversations/$id/responses/$requestId") { client.get(it) { authorise() } }.body<ResponseToEdit>()
        } catch (gone: ApiException) {
            if (gone.status == 404) null else throw gone
        }

    suspend fun filterResponse(id: String, requestId: String, filter: String): ResponseFilterResult =
        call("/conversations/$id/responses/$requestId/filter") { client.post(it) { authorise(); json(ResponseFilterRequest(filter)) } }.body()

    suspend fun sendResponse(id: String, requestId: String, send: JsonElement) {
        call("/conversations/$id/responses/$requestId") { client.post(it) { authorise(); json(SendResponseRequest(send)) } }
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
