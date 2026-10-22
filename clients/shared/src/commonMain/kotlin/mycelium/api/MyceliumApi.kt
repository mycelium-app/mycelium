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
