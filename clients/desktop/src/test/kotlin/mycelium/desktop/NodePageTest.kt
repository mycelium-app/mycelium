package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
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
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun nodeJson(online: Boolean = true) = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.1.10","address":"10.0.0.5",
"online":$online,"draining":false,"sessions":["c1"],"agents":[{"agent":"claude","version":"@agentclientprotocol/claude-agent-acp 0.94.0","available":true}],"logins":[]}]"""

private const val Conversation = """[{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/backups","title":"Backups","status":"working","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}]"""

private const val Sample = """{"at_ms":12000,"available":true,"cpu_percent":38.4,"cores":8,"load":[0.52,0.4,0.31],
"memory_total_bytes":32000000000,"memory_available_bytes":10600000000,"swap_total_bytes":0,"swap_free_bytes":0,
"disks":[{"mount":"/","total_bytes":140000000000,"free_bytes":13000000000}],
"processes":[{"pid":10,"name":"gradle","command":"gradle","cpu_percent":212.0,"memory_bytes":3100000000,"stoppable":true},
{"pid":11,"name":"node","command":"claude-agent-acp","cpu_percent":14.0,"memory_bytes":640000000,"agent":true,"conversation_id":"c1"}],
"ports":[{"port":3000,"address":"0.0.0.0","pid":11,"process":"node","conversation_id":"c1","cwd":"/work/backups"},
{"port":5432,"address":"0.0.0.0","pid":12,"process":"postgres"},
{"port":8080,"address":"127.0.0.1","pid":13,"process":"dev"}],
"docker":{"running":2,"containers":[{"name":"db","state":"running"},{"name":"cache","state":"running"},{"name":"old-web","state":"exited"}]}}"""

private object NodePagePlatform : Platform {
    var opened: String? = null

    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) {
        opened = url
    }
    override fun copyToClipboard(text: String) = Unit
}

class NodePageTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val stopped = CopyOnWriteArrayList<String>()

    private fun showNodes(nodes: String, watched: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        stopped.clear()
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post && path.endsWith("/stop")) {
                stopped += path

                return@MockEngine respond("", HttpStatusCode.NoContent)
            }

            val body = when {
                request.method == HttpMethod.Post && path.endsWith("/watch") -> watched
                path.endsWith("/nodes") -> nodes
                path.endsWith("/conversations") -> Conversation
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        NodePagePlatform.opened = null
        rule.setContent { MyceliumTheme(true) { Shell(store, NodePagePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Nodes) } }
    }

    private fun openNode(tab: String) {
        rule.onNodeWithContentDescription("Open willow").performClick()
        rule.onNodeWithText(tab).performClick()
        rule.waitForIdle()
    }

    @Test
    fun selectingANodeOpensItsPageWithAgentsResourcesAndPorts() {
        showNodes(nodeJson(), """{"latest":$Sample,"history":[]}""")

        rule.onNodeWithContentDescription("Open willow").performClick()

        rule.onNodeWithText("Resources").assertExists()
        rule.onNodeWithText("Ports").assertExists()
        rule.onNodeWithText("Conversations running here").assertExists()
        rule.onNodeWithContentDescription("Open Backups").assertExists()
    }

    @Test
    fun resourcesShowCpuMemoryDiskLoadAndTheBusiestProcessesWithTheirConversation() {
        showNodes(nodeJson(), """{"latest":$Sample,"history":[{"at_ms":0,"cpu_percent":10.0},{"at_ms":300000,"cpu_percent":70.0}]}""")

        openNode("Resources")

        rule.onNodeWithText("38%").assertExists()
        rule.onNodeWithText("21.4 GB of 32 GB").assertExists()
        rule.onNodeWithText("13 GB free").assertExists()
        rule.onNodeWithText("8 cores  ·  load 0.52").assertExists()
        rule.onNodeWithText("212%").assertExists()
        rule.onNodeWithText("claude-agent-acp · Backups").assertExists()
        rule.onNodeWithContentDescription("CPU over the last 5 minutes. Average 40%, peak 70%.").assertExists()
    }

    @Test
    fun theDockerCardCountsContainersAndListsEachWithItsState() {
        showNodes(nodeJson(), """{"latest":$Sample,"history":[]}""")

        openNode("Resources")

        rule.onNodeWithText("3 containers").assertExists()
        rule.onNodeWithText("2 running").assertExists()
        rule.onNodeWithText("old-web").assertExists()
        rule.onNodeWithText("exited").assertExists()
    }

    @Test
    fun theDockerCardShowsHowMuchCanBeReclaimedWhenTheNodeReportsIt() {
        showNodes(nodeJson(), """{"latest":${Sample.replace(""""running":2,""", """"running":2,"reclaimable_bytes":21800000000,""")},"history":[]}""")

        openNode("Resources")

        rule.onNodeWithText("21.8 GB reclaimable").assertExists()
    }

    @Test
    fun aNodeWithoutDockerSaysSoAndTheRestOfThePageStillShows() {
        showNodes(nodeJson(), """{"latest":${Sample.substringBefore(",\n\"docker\"")}},"history":[]}""")

        openNode("Resources")

        rule.onNodeWithText("Not available").assertExists()
        rule.onNodeWithText("Docker is not installed on this node, or its service is not running.").assertExists()
        rule.onNodeWithText("38%").assertExists()
    }

    @Test
    fun stoppingAProcessNamesItInAConfirmationAndOnlyThenAsksTheNode() {
        showNodes(nodeJson(), """{"latest":$Sample,"history":[]}""")
        openNode("Resources")

        rule.onNodeWithContentDescription("Stop gradle").performClick()
        rule.onNodeWithText("Stop gradle?").assertExists()
        assertEquals(emptyList<String>(), stopped)

        rule.onNodeWithText("Stop gradle").performClick()
        rule.waitForIdle()

        assertEquals(listOf("/api/v1/nodes/n1/processes/10/stop"), stopped)
        rule.onNodeWithText("Asked gradle to stop. It leaves this list once it has exited.").assertExists()
    }

    @Test
    fun cancellingTheConfirmationStopsNothing() {
        showNodes(nodeJson(), """{"latest":$Sample,"history":[]}""")
        openNode("Resources")

        rule.onNodeWithContentDescription("Stop gradle").performClick()
        rule.onNodeWithText("Cancel").performClick()
        rule.waitForIdle()

        assertEquals(emptyList<String>(), stopped)
        assertEquals(0, rule.onAllNodesWithText("Stop gradle?").fetchSemanticsNodes().size)
    }

    @Test
    fun aProcessTheNodeDoesNotOwnHasNoStopAction() {
        showNodes(nodeJson(), """{"latest":$Sample,"history":[]}""")
        openNode("Resources")

        assertEquals(1, rule.onAllNodes(androidx.compose.ui.test.hasContentDescription("Stop", substring = true)).fetchSemanticsNodes().size)
    }

    @Test
    fun portsListTheOwningProcessAndConversationAndOnlyOfferOpenForReachableWebPorts() {
        showNodes(nodeJson(), """{"latest":$Sample,"history":[]}""")

        openNode("Ports")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescriptionCount("Open port 3000 on willow in the browser") == 1 }
        rule.onNodeWithContentDescription("Open port 3000 on willow in the browser").performClick()

        assertEquals("http://10.0.0.5:3000", NodePagePlatform.opened)
        assertEquals(true, rule.onAllNodesWithText("Backups").fetchSemanticsNodes().isNotEmpty())
        rule.onNodeWithText("postgres").assertExists()
        rule.onNodeWithText("Local only").assertExists()
        assertEquals(0, rule.onAllNodesWithContentDescriptionCount("Open port 5432 on willow in the browser"))
        assertEquals(0, rule.onAllNodesWithContentDescriptionCount("Open port 8080 on willow in the browser"))
    }

    @Test
    fun aNodeThatCannotReadItsResourcesSaysSoInsteadOfShowingZeros() {
        showNodes(nodeJson(), """{"latest":{"at_ms":1,"available":false},"history":[]}""")

        openNode("Resources")

        rule.onNodeWithText("Resources are not available on this node. Mycelium reads them from /proc, which only Linux provides.").assertExists()
    }

    @Test
    fun anOfflineNodeExplainsWhyThereAreNoResources() {
        showNodes(nodeJson(online = false), """{"latest":null,"history":[]}""")

        openNode("Ports")

        rule.onNodeWithText("This node is offline. Resources and ports appear here when it connects.").assertExists()
    }

    @Test
    fun anOnlineNodeWithNoSampleYetSaysItIsWaiting() {
        showNodes(nodeJson(), """{"latest":null,"history":[]}""")

        openNode("Resources")

        rule.onNodeWithText("Waiting for the first sample from willow.").assertExists()
    }
}

private fun androidx.compose.ui.test.junit4.ComposeContentTestRule.onAllNodesWithContentDescriptionCount(description: String): Int =
    onAllNodes(androidx.compose.ui.test.hasContentDescription(description)).fetchSemanticsNodes().size
