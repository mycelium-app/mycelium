package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
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
import mycelium.api.EnvironmentAction
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.NodeSummary
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.ui.EnvironmentPanel
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Check the harbour sign-in","status":"working","acp_session_id":"s","created_at":"2026-10-28T10:00:00Z","updated_at":"2026-10-28T10:00:00Z"}"""

private const val CanRun = """{"available":true,"versions":{"compositor":"sway 1.10","wayvnc":"0.9.1","browser":"Chromium 141"}}"""

private const val CannotRun = """{"available":false,"missing":["sway","wayvnc"]}"""

private fun nodeJson(id: String, name: String, environment: String, online: Boolean = true) =
    """{"id":"$id","name":"$name","hostname":"$name","os":"linux","arch":"x86_64","version":"v0.14.0","online":$online,"draining":false,"environment":$environment}"""

private fun environmentJson(state: String = "running", controller: String = "agent", extra: String = "") =
    """{"type":"environment","conversation_id":"c1","state":"$state","controller":"$controller","node_id":"n1","node_name":"atlas-box","size":{"width":1280,"height":800},"started_at":"2026-10-28T10:00:00Z","browser":"Chromium 141"$extra}"""

private class EnvironmentPlatform(override val showsLiveView: Boolean = false) : Platform {
    var liveUrl: String? = null
    var liveViewOnly: Boolean? = null

    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection() = Connection("https://mycelium.test", "token")
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
    override fun localDateTime(epochMillis: Long): String? = "2026-10-28T14:32:00"

    @Composable
    override fun LiveView(url: String, viewOnly: Boolean, modifier: Modifier, onPointer: () -> Unit) {
        liveUrl = url
        liveViewOnly = viewOnly
        Box(modifier)
    }
}

class EnvironmentPanelTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<String>()
    private var expanded by mutableStateOf(false)

    private fun show(
        environment: String? = null,
        nodes: List<String> = listOf(nodeJson("n1", "atlas-box", CanRun)),
        actions: List<EnvironmentAction> = emptyList(),
        platform: EnvironmentPlatform = EnvironmentPlatform(),
    ) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            if (request.url.encodedPath.endsWith("/stream-ticket")) {
                return@MockEngine respond("""{"ticket":"token"}""", HttpStatusCode.OK, json)
            }

            requests += "${request.method.value} ${request.url.encodedPath}"

            respond("{}", HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        val conversation = MyceliumJson.decodeFromString(Conversation.serializer(), ConversationJson)
        val parsed = nodes.map { MyceliumJson.decodeFromString(NodeSummary.serializer(), it) }
        val state = AppState(
            conversations = mapOf("c1" to conversation),
            nodes = parsed.associateBy { it.id },
            environments = environment?.let { mapOf("c1" to (parseStreamMessage(it) as StreamMessage.EnvironmentChanged).environment) }.orEmpty(),
            environmentActions = mapOf("c1" to actions),
        )

        expanded = false
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalPlatform provides platform) {
                    EnvironmentPanel(store, state, conversation, expanded, { expanded = it }, {}, {}, {}, {})
                }
            }
        }
    }

    @Test
    fun withNoEnvironmentThePanelOffersToStartOne() {
        show()

        rule.onNodeWithText("No environment yet").assertExists()
        rule.onNodeWithText("Start environment").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertEquals(listOf("POST /api/v1/conversations/c1/environment"), requests.toList())
    }

    @Test
    fun whileStartingThePanelNamesTheNodeAndCancelStopsIt() {
        show(environmentJson("starting"))

        rule.onNodeWithText("Starting the environment on atlas-box").assertExists()
        rule.onNodeWithText("Cancel").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertTrue("DELETE /api/v1/conversations/c1/environment" in requests)
    }

    @Test
    fun aFailedStartShowsWhatFailedAndTryAgainStartsAgain() {
        show(environmentJson("failed", extra = ""","failure":"wayvnc exited before the session was ready""""))

        rule.onNodeWithText("wayvnc exited before the session was ready").assertExists()
        rule.onNodeWithText("Try again").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertEquals("POST /api/v1/conversations/c1/environment", requests.first())
    }

    @Test
    fun whenTheAgentIsAtWorkThePanelSaysSoAndOffersTakeOver() {
        show(environmentJson(extra = ""","target":{"x":640,"y":400,"label":"Click, Password"}"""), actions = listOf(EnvironmentAction("2026-10-28T10:00:05Z", "Clicking “Password”")))

        rule.onNodeWithText("Agent in control").assertExists()
        rule.onNodeWithText("1280 × 800").assertExists()
        rule.onNodeWithText("Clicking “Password”").assertExists()
        rule.onNodeWithContentDescription("Actions, newest first").assertExists()
        rule.onNodeWithText("Take over").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertEquals(listOf("POST /api/v1/conversations/c1/environment/take-over"), requests.toList())
    }

    @Test
    fun theLiveViewIsAnImageWithATextDescriptionAndIsViewOnlyWhileTheAgentWorks() {
        val platform = EnvironmentPlatform(showsLiveView = true)

        show(environmentJson(extra = ""","target":{"x":640,"y":400,"label":"Click, Password"}"""), platform = platform)

        rule.waitUntil(5_000) { platform.liveViewOnly != null }
        rule.onNodeWithContentDescription("Live view of the environment on atlas-box. The agent's pointer is at Click, Password.").assertExists()
        assertEquals(true, platform.liveViewOnly)
        assertEquals("https://mycelium.test/novnc/embed.html?path=%2Fapi%2Fv1%2Fconversations%2Fc1%2Fenvironment%2Fstream&ticket=token", platform.liveUrl)
    }

    @Test
    fun withoutAWebViewThePanelSaysTheLiveViewIsInTheDesktopApp() {
        show(environmentJson())

        rule.onNodeWithText("The live view is available in the desktop app.").assertExists()
        rule.onNodeWithText("Take over").assertExists()
    }

    @Test
    fun whenThePersonIsInControlThePanelOffersHandBackAndSaysTheAgentIsPaused() {
        val platform = EnvironmentPlatform(showsLiveView = true)

        show(environmentJson(controller = "person"), platform = platform)

        rule.waitUntil(5_000) { platform.liveViewOnly != null }
        rule.onNodeWithText("You are in control").assertExists()
        rule.onNodeWithText("Take over").assertDoesNotExist()
        rule.onNodeWithText("The agent is paused and knows you took over.", substring = true).assertExists()
        assertEquals(false, platform.liveViewOnly)

        rule.onNodeWithText("Hand back").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertEquals(listOf("POST /api/v1/conversations/c1/environment/hand-back"), requests.toList())
    }

    @Test
    fun stoppingFromTheBarDeletesTheEnvironment() {
        show(environmentJson())

        rule.onNodeWithContentDescription("Stop the environment").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertTrue("DELETE /api/v1/conversations/c1/environment" in requests)
    }

    @Test
    fun expandingFillsTheConversationAndBackToConversationHandsTheAreaBack() {
        show(environmentJson())

        rule.onNodeWithContentDescription("Expand the environment").performClick()
        rule.waitForIdle()

        assertTrue(expanded)
        rule.onNodeWithContentDescription("Environment, expanded").assertExists()
        rule.onNodeWithContentDescription("Expand the environment").assertDoesNotExist()
        rule.onNodeWithContentDescription("Close the environment panel").assertDoesNotExist()
        rule.onNodeWithText("Take over").assertExists()
        rule.onNodeWithContentDescription("Stop the environment").assertExists()

        rule.onNodeWithContentDescription("Back to conversation").performClick()
        rule.waitForIdle()

        assertFalse(expanded)
        rule.onNodeWithContentDescription("Expand the environment").assertExists()
    }

    @Test
    fun aStoppedEnvironmentSaysWhenAndWarnsThatSignedInSessionsAreGone() {
        show(environmentJson("stopped", extra = ""","stopped_at":"2026-10-28T14:32:10Z""""))

        rule.onNodeWithText("Environment stopped").assertExists()
        rule.onNodeWithText("You stopped it at 14:32. The browser closed with it, so anything signed in there is signed out.", substring = true).assertExists()
        rule.onNodeWithText("Start environment").assertExists()
    }

    @Test
    fun aNodeThatCannotRunOneExplainsAndOffersAnotherNodeAndCheckAgain() {
        show(nodes = listOf(nodeJson("n1", "laptop", CannotRun), nodeJson("n2", "atlas-box", CanRun)))

        rule.onNodeWithText("laptop cannot run an environment").assertExists()
        rule.onNodeWithText("sway and wayvnc are not installed on laptop", substring = true).assertExists()
        rule.onNodeWithText("Or move this conversation to atlas-box, which can run one.").assertExists()
        rule.onNodeWithText("Move to atlas-box").assertExists()
        rule.onNodeWithText("laptop node page").assertExists()
        rule.onNodeWithText("Check again").performClick()
        rule.waitUntil(5_000) { requests.isNotEmpty() }

        assertEquals(listOf("POST /api/v1/nodes/n1/environment/check"), requests.toList())
    }

    @Test
    fun withNoOtherNodeThatCanRunOneThePanelDoesNotOfferToMove() {
        show(nodes = listOf(nodeJson("n1", "laptop", CannotRun)))

        rule.onNodeWithText("Move to atlas-box").assertDoesNotExist()
        rule.onNodeWithText("Check again").assertExists()
    }
}
