package mycelium.desktop

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockRequestHandleScope
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.client.request.HttpResponseData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.state.Store

internal class AppsPlatform : Platform {
    val opened = mutableListOf<String>()
    val copied = mutableListOf<String>()
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) {
        opened += url
    }

    override fun copyToClipboard(text: String) {
        copied += text
    }
}

internal const val CoolifyAccount = """{"id":"c1","name":"Coolify","kind":"coolify","url":"https://coolify.example.com","enabled":true,"status":"connected","tools":[],"created_at":"2026-10-27T09:00:00Z","policy":{"preset":"ask_writes"},"settings":{"project_uuid":"p1","project_name":"Mycelium apps","server_uuid":"s1","server_name":"atlas-box","wildcard_domain":"apps.example.com"}}"""

internal const val AppDetailJson = """{"uuid":"a1","name":"harbour-status","status":"running","address":"https://harbour-status.apps.example.com","repository":"git.example.com/sam/harbour","branch":"main","base_directory":"apps/status","build_pack":"dockerfile","project":"Mycelium apps","server":"atlas-box","conversation_id":null,"created_at":"2026-10-21T10:00:00Z","latest_deployment":{"uuid":"d1","started_at":"2026-10-28T14:02:00Z","commit":"4f2c1a9d3e","commit_message":"Add the uptime chart","triggered_by":"agent","conversation_id":null,"duration_seconds":108,"result":"deployed"}}"""

internal const val DeploymentsJson = """[{"uuid":"d1","started_at":"2026-10-28T14:02:00Z","commit":"4f2c1a9d3e","commit_message":"Add the uptime chart","triggered_by":"agent","conversation_id":null,"duration_seconds":108,"result":"deployed"},{"uuid":"d0","started_at":"2026-10-28T11:12:00Z","commit":"c81d2f0aa1","commit_message":"First cut of the status page","triggered_by":"person","conversation_id":null,"duration_seconds":41,"result":"failed"}]"""

internal const val EnvsJson = """[{"key":"DATABASE_URL","secret":true,"build_time":false,"value":null,"changed_at":"2026-10-28T12:00:00Z","changed_by":"agent","stale":true},{"key":"PORT","secret":false,"build_time":false,"value":"8080","changed_at":"2026-10-26T12:00:00Z","changed_by":"person","stale":false}]"""

internal const val LogsJson = """{"lines":["14:03:51 INFO Listening on :8080","14:31:12 ERROR Could not reach the backup database"],"available":true,"message":null}"""

internal val JsonHeaders = headersOf(HttpHeaders.ContentType, "application/json")

internal fun appsStore(scope: CoroutineScope, handler: suspend MockRequestHandleScope.(HttpRequestData) -> HttpResponseData): Store =
    Store(MyceliumApi("https://mycelium.test", "token", MockEngine { request -> this.handler(request) }), scope)

internal fun MockRequestHandleScope.json(body: String, status: HttpStatusCode = HttpStatusCode.OK): HttpResponseData = respond(body, status, JsonHeaders)

internal fun MockRequestHandleScope.respond204(): HttpResponseData = respond("", HttpStatusCode.NoContent)
