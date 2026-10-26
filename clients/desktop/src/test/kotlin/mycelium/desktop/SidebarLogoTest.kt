package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private const val LogoChat = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Backups","status":"working","project_id":"p1","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""
private const val LogoNode = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.1.10","address":"10.0.0.5","online":true,"draining":false,"sessions":["c1"],"agents":[{"agent":"claude","available":true}],"logins":[]}]"""
private const val LogoProject = """{"id":"p1","node_id":"n1","name":"app","path":"/work/app","avatar_sha256":"logo-sha","created_at":"2026-10-22T10:00:00Z"}"""

private object LogoPlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

class SidebarLogoTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    @Test
    fun theSidebarLoadsAProjectsUploadedLogo() {
        val fetched = CopyOnWriteArrayList<String>()
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            fetched += path
            val body = when {
                path.endsWith("/attachments/logo-sha") -> return@MockEngine respond(ByteArray(0), HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "image/png"))
                path.endsWith("/conversations") && request.url.parameters["archived"] != "true" -> "[$LogoChat]"
                path.endsWith("/projects") -> "[$LogoProject]"
                path.endsWith("/nodes") -> LogoNode
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, LogoPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Backups", substring = true).fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()

        assertTrue(fetched.any { it.endsWith("/attachments/logo-sha") }, fetched.distinct().joinToString())
    }
}
