package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.NodesSettings
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.api.StreamMessage
import androidx.compose.ui.test.performScrollTo
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val NodesJson = """[
{"id":"n1","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"v0.34.50","update_available":"v0.34.56","address":"192.0.2.10","online":true,"draining":false,
"agents":[{"agent":"claude","version":"@agentclientprotocol/claude-agent-acp 0.94.0","available":true,"update_available":"0.108.1"},
{"agent":"codex","version":"@agentclientprotocol/codex-acp 2.2.2","available":true},
{"agent":"vibe","available":false}],
"logins":[{"agent":"claude","account":"sam@example.com","method":"oauth_token","state":"signed_in"}]},
{"id":"n2","name":"orion","hostname":"orion","os":"linux","arch":"aarch64","version":"v0.34.56","address":"192.0.2.11","online":false,"draining":false,
"agents":[{"agent":"claude","version":"@agentclientprotocol/claude-agent-acp 0.108.1","available":true}],"logins":[]}]"""

private const val UpToDateJson = """[{"id":"n3","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.34.56","address":"192.0.2.12","online":true,"draining":false,
"agents":[{"agent":"claude","version":"@agentclientprotocol/claude-agent-acp 0.108.1","available":true}],"logins":[]}]"""

private object NodesSettingsPlatform : Platform {
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

class NodesSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val updates = java.util.concurrent.CopyOnWriteArrayList<String>()
    private var updateRefusal: String? = null

    private fun store(nodes: String = NodesJson): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post && path.endsWith("/update")) {
                updates += path
                val refusal = updateRefusal

                return@MockEngine if (refusal == null) respond("""{"update_id":"u1"}""", HttpStatusCode.OK, json) else respond("""{"error":"agent_update_refused","message":"$refusal"}""", HttpStatusCode.Conflict, json)
            }

            val body = when {
                request.method == HttpMethod.Post && path.endsWith("/watch") -> """{"history":[]}"""
                path.endsWith("/nodes") -> nodes
                path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        return store
    }

    private fun showPage(nodes: String = NodesJson, onOpenNode: (String) -> Unit = {}): Store {
        val store = store(nodes)
        rule.setContent { MyceliumTheme(true) { NodesSettings(store, NodesSettingsPlatform, onOpenNode) } }
        rule.waitForIdle()

        return store
    }

    private fun showShell(route: Route = Route.Inbox): Store {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, NodesSettingsPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }
        rule.waitForIdle()

        return store
    }

    @Test
    fun eachNodeShowsItsStateItsMyceliumVersionAndEachAgentsVersion() {
        showPage()

        rule.onNodeWithText("Online").assertExists()
        rule.onNodeWithText("Offline").assertExists()
        rule.onNodeWithText("v0.34.56").assertExists()
        rule.onNodeWithText("2.2.2").assertExists()
        rule.onNodeWithText("0.108.1").assertExists()
    }

    @Test
    fun anAgentMissingFromANodeIsNotInstalled() {
        showPage()

        assertEquals(3, rule.onAllNodesWithText("Not installed", useUnmergedTree = true).fetchSemanticsNodes().size)
    }

    @Test
    fun anAvailableUpdateReadsAsTheOldVersionThenTheNewOne() {
        showPage()

        rule.onNodeWithContentDescription("0.94.0, update to 0.108.1").assertExists()
        rule.onNodeWithContentDescription("v0.34.50, update to v0.34.56").assertExists()
    }

    @Test
    fun thePageOffersJoinANodeAndNoSignIns() {
        showPage()

        rule.onNodeWithText("Join a node").assertExists()
        rule.onNodeWithText("Sign-ins live in Accounts. Select a node for its agents, resources and ports.").assertExists()
        assertEquals(0, rule.onAllNodesWithText("sam@example.com").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Log in", substring = true).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Add login").fetchSemanticsNodes().size)
    }

    @Test
    fun joinANodeOpensTheJoinFlow() {
        showPage(UpToDateJson)

        rule.onNodeWithText("Join a node").performClick()

        rule.onNodeWithText("Waiting for the next node").assertExists()
    }

    @Test
    fun selectingARowOpensThatNode() {
        var opened: String? = null
        showPage(onOpenNode = { opened = it })

        rule.onNodeWithContentDescription("Open orion").performClick()

        assertEquals("n2", opened)
    }

    @Test
    fun aNodeOpensInsideSettingsWithABackLinkToNodes() {
        showShell(Route.Settings)

        rule.onNodeWithText("Nodes").performClick()
        rule.onNodeWithContentDescription("Open atlas").performClick()
        rule.waitForIdle()

        rule.onNodeWithText("Ports").assertExists()
        rule.onNodeWithContentDescription("Back to Nodes").performClick()
        rule.waitForIdle()

        rule.onNodeWithText("Join a node").assertExists()
    }

    @Test
    fun theOldNodesAddressOpensNodesInSettings() {
        val store = showShell()

        rule.runOnIdle { store.openScreen("nodes") }
        rule.waitForIdle()

        rule.onNodeWithText("Sign-ins live in Accounts. Select a node for its agents, resources and ports.").assertExists()
    }

    @Test
    fun theSidebarHasNoNodesButton() {
        showShell(Route.New)

        assertEquals(0, rule.onAllNodesWithContentDescription("Nodes").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Nodes").fetchSemanticsNodes().size)
    }

    @Test
    fun updateAgentsShowsOnlyOnAnOnlineNodeWithAnUpdateAndStreamsItsProgress() {
        val store = showPage()

        assertEquals(1, rule.onAllNodesWithText("Update agents").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Update the agents on atlas").performScrollTo().performClick()

        rule.waitUntil(5_000) { updates.isNotEmpty() }
        assertEquals(listOf("/api/v1/nodes/n1/agents/claude/update"), updates)
        rule.onNodeWithText("Updating").assertExists()

        rule.runOnIdle { store.receive(StreamMessage.AgentUpdateOutput("u1", "added 1 package in 3s\r\n")) }
        rule.onNodeWithText("added 1 package in 3s").assertExists()

        rule.runOnIdle { store.receive(StreamMessage.AgentUpdateFinished("u1", true)) }
        rule.onNodeWithText("Updated").assertExists()
    }

    @Test
    fun anUpdateThatFailsSaysSoWithItsLastLine() {
        val store = showPage()
        rule.onNodeWithContentDescription("Update the agents on atlas").performScrollTo().performClick()
        rule.waitUntil(5_000) { updates.isNotEmpty() }

        rule.runOnIdle {
            store.receive(StreamMessage.AgentUpdateOutput("u1", "npm error code EACCES\r\n"))
            store.receive(StreamMessage.AgentUpdateFinished("u1", false))
        }

        rule.onNodeWithText("Failed").assertExists()
        rule.onNodeWithText("npm error code EACCES").assertExists()
    }

    @Test
    fun anUpdateTheNodeRefusesShowsTheReason() {
        updateRefusal = "Claude Code on this node comes from Nix, so Mycelium cannot update it."
        showPage()
        rule.onNodeWithContentDescription("Update the agents on atlas").performScrollTo().performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Failed").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Claude Code on this node comes from Nix, so Mycelium cannot update it.", substring = true).assertExists()
    }

    @Test
    fun aNodeWithEverythingUpToDateOffersNoUpdate() {
        showPage(UpToDateJson)

        assertEquals(0, rule.onAllNodesWithText("Update agents").fetchSemanticsNodes().size)
    }
}
