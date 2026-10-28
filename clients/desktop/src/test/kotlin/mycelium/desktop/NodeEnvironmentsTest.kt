package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.NodeSummary
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.NodePage
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.time.Clock

private const val RunningConversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Check the harbour sign-in","status":"working","acp_session_id":"s","created_at":"2026-10-28T10:00:00Z","updated_at":"2026-10-28T10:00:00Z"}"""

private object NodeEnvironmentsPlatform : Platform {
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

class NodeEnvironmentsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<String>()

    private fun showNode(environment: String, os: String = "linux") {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            requests += "${request.method.value} ${request.url.encodedPath}"

            respond("{}", HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        val node = MyceliumJson.decodeFromString(
            NodeSummary.serializer(),
            """{"id":"n1","name":"atlas-box","hostname":"atlas-box","os":"$os","arch":"x86_64","version":"v0.14.0","online":true,"draining":false,"environment":$environment}""",
        )
        val state = AppState(nodes = mapOf("n1" to node), conversations = mapOf("c1" to MyceliumJson.decodeFromString(Conversation.serializer(), RunningConversation)))

        rule.setContent { MyceliumTheme(true) { NodePage(store, state, "n1", NodeEnvironmentsPlatform, onBack = {}, onOpenConversation = {}) } }
        rule.onNodeWithText("Resources").performClick()
        rule.waitForIdle()
    }

    @Test
    fun aNodeThatCanRunEnvironmentsListsTheToolsItFoundAndWhatIsRunning() {
        val started = Clock.System.now().minus(kotlin.time.Duration.parse("12m"))

        showNode(
            """{"available":true,"versions":{"compositor":"sway 1.10","wayvnc":"0.9.1","browser":"Chromium 141"},"running":[{"conversation_id":"c1","controller":"agent","started_at":"$started","memory_bytes":410000000}]}""",
        )

        rule.onNodeWithText("Environments").assertExists()
        rule.onNodeWithText("Can run environments  ·  sway 1.10, wayvnc 0.9.1, Chromium 141").assertExists()
        rule.onNodeWithText("Check the harbour sign-in").assertExists()
        rule.onNodeWithText("Agent in control").assertExists()
        rule.onNodeWithText("12 min").assertExists()
        rule.onNodeWithText("410 MB").assertExists()
    }

    @Test
    fun stoppingARunningEnvironmentFromTheNodePageDeletesItForThatConversation() {
        showNode("""{"available":true,"running":[{"conversation_id":"c1","controller":"person","started_at":"2026-10-28T10:00:00Z"}]}""")

        rule.onNodeWithContentDescription("Stop the environment of Check the harbour sign-in").performClick()
        rule.waitUntil(5_000) { requests.any { it.startsWith("DELETE") } }

        assertEquals(listOf("DELETE /api/v1/conversations/c1/environment"), requests.filter { it.startsWith("DELETE") })
    }

    @Test
    fun aNodeThatCannotRunEnvironmentsSaysWhatIsMissingAndChecksAgain() {
        showNode("""{"available":false,"missing":["sway","wayvnc"]}""")

        rule.onNodeWithText("Cannot run environments: sway and wayvnc are not installed.").assertExists()
        rule.onNodeWithText("Install them with the package manager on atlas-box, then check again.").assertExists()
        rule.onNodeWithText("Check again").performClick()
        rule.waitUntil(5_000) { requests.any { it.contains("/environment/") } }

        assertEquals(listOf("POST /api/v1/nodes/n1/environment/check"), requests.filter { it.contains("/environment/") })
    }

    @Test
    fun aNodeThatNeverRunsEnvironmentsShowsItsNoteAndNothingToInstall() {
        showNode("""{"available":false,"note":"Environments need a Linux node"}""", os = "macos")

        rule.onNodeWithText("Environments need a Linux node").assertExists()
        rule.onNodeWithText("Check again").assertDoesNotExist()
    }
}
