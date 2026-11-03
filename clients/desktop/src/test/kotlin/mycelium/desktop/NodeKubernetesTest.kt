package mycelium.desktop

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.content.TextContent
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ExperimentsSettings
import mycelium.ui.MyceliumTheme
import mycelium.ui.NodePage
import mycelium.ui.NodesScreen
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Cluster = """[{"id":"k1","name":"lab-cluster","hostname":"lab-cluster","os":"linux","arch":"x86_64","version":"v0.1.10","online":true,"draining":false,"sessions":["c1","c2","c3"],"agents":[],"logins":[],
"kubernetes":{"outpost":"lab-cluster","namespace":"team-a","image":"ghcr.io/mycelium-app/mycelium-agent:latest","credentials_secret":"mycelium-agent-credentials","workspace_persistent":false,"pods":[
{"conversation_id":"c1","name":"mycelium-session-7f3c2a91","state":"running"},
{"conversation_id":"c2","name":"mycelium-session-b41e08d2","state":"running"},
{"conversation_id":"c3","name":"mycelium-session-52d9c6e0","state":"exited"}]}}]"""

private const val Conversations = """[
{"id":"c1","owner":"u","node_id":"k1","agent":"codex","cwd":"/workspace/ledger","title":"Uploads survive a 503 retry","status":"working","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"},
{"id":"c2","owner":"u","node_id":"k1","agent":"claude","cwd":"/workspace/atlas","title":"Release v0.3.7 checklist","status":"input","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"},
{"id":"c3","owner":"u","node_id":"k1","agent":"codex","cwd":"/workspace/fern","title":"Tidy the nginx config","status":"done","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}]"""

private const val LaptopNode = """[{"id":"n1","name":"orion","hostname":"orion","os":"linux","arch":"x86_64","version":"v0.1.10","online":true,"draining":false,"agents":[],"logins":[]}]"""

private object KubernetesPlatform : Platform {
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

class NodeKubernetesTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val saved = CopyOnWriteArrayList<String>()
    private val tokenRequests = CopyOnWriteArrayList<String>()

    private fun store(experiments: String, nodes: String): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request: HttpRequestData ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Put && path.endsWith("/experiments")) {
                val sent = (request.body as TextContent).text
                saved += sent

                return@MockEngine respond(sent, headers = json)
            }

            val body = when {
                path.endsWith("/nodes/join-tokens") -> {
                    tokenRequests += (request.body as TextContent).text

                    """{"token":"myc_join_example","expires_at":"2026-11-03T11:00:00Z"}"""
                }
                path.endsWith("/experiments") -> experiments
                path.endsWith("/nodes") -> nodes
                path.endsWith("/conversations") -> Conversations
                else -> "{}"
            }

            respond(body, headers = json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking {
            store.refreshExperiments()
            store.refresh()
        }
        saved.clear()

        return store
    }

    private fun showJoinDialog(experiments: String) {
        val store = store(experiments, LaptopNode)
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NodesScreen(store, state, KubernetesPlatform) } }
        rule.onNodeWithContentDescription("Join a node").performClick()
        rule.waitForIdle()
    }

    private fun present(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun turningOnKubernetesNodesSavesItWithoutTurningVoiceOff() {
        val store = store("""{"voice":true}""", LaptopNode)
        rule.setContent { MyceliumTheme(true) { ExperimentsSettings(store) } }

        rule.onNodeWithContentDescription("Kubernetes nodes").performClick()
        rule.waitUntil(5_000) { saved.isNotEmpty() }

        assertTrue(saved.single().contains("\"voice\":true") && saved.single().contains("\"kubernetes_nodes\":true"), saved.single())
    }

    @Test
    fun turningOnVoiceKeepsKubernetesNodesOn() {
        val store = store("""{"kubernetes_nodes":true}""", LaptopNode)
        rule.setContent { MyceliumTheme(true) { ExperimentsSettings(store) } }

        rule.onNodeWithContentDescription("Voice input").performClick()
        rule.waitUntil(5_000) { saved.isNotEmpty() }

        assertTrue(saved.single().contains("\"voice\":true") && saved.single().contains("\"kubernetes_nodes\":true"), saved.single())
    }

    @Test
    fun theJoinDialogOffersKubernetesOnlyWithTheExperimentOn() {
        showJoinDialog("""{"kubernetes_nodes":false}""")

        assertTrue(present("Shell"))
        assertEquals(false, present("Kubernetes"))
    }

    private fun openAddAnOutpost() {
        showJoinDialog("""{"kubernetes_nodes":true}""")
        rule.onNodeWithText("Kubernetes").performClick()
        rule.waitForIdle()
    }

    @Test
    fun addingAnOutpostAsksForANameANamespaceAndAStorageClassBeforeShowingCommands() {
        openAddAnOutpost()

        assertTrue(present("Add an outpost"))
        rule.onNodeWithContentDescription("Name").assertExists()
        rule.onNodeWithContentDescription("Namespace").assertExists()
        rule.onNodeWithContentDescription("Storage class").assertExists()
        assertEquals(false, present("Create the outpost in the cluster"))
    }

    private fun fillInAnOutpost() {
        openAddAnOutpost()
        rule.onNodeWithContentDescription("Name").performTextInput("lab-cluster")
        rule.onNodeWithContentDescription("Namespace").performTextReplacement("team-a")
        rule.onNodeWithContentDescription("Storage class").performTextInput("fast")
        rule.waitForIdle()
    }

    @Test
    fun anOutpostWithANameAndNamespaceOffersAJoinTokenBeforeAnyCommand() {
        fillInAnOutpost()

        assertTrue(present("Create a join token"))
        rule.onNodeWithContentDescription("Create join token").assertExists()
        assertEquals(false, present("Create the outpost in the cluster"))
        assertTrue(tokenRequests.isEmpty())
    }

    @Test
    fun creatingAJoinTokenForTheNameShowsItAndTheCopyableStepsThatUseIt() {
        fillInAnOutpost()
        rule.onNodeWithContentDescription("Create join token").performClick()
        rule.waitUntil(5_000) { present("Create the outpost in the cluster") }

        assertEquals(listOf("""{"name":"lab-cluster"}"""), tokenRequests.toList())
        assertTrue(present("myc_join_example"))

        for (title in listOf("Create the outpost in the cluster", "Add agent sign-ins")) {
            assertTrue(present(title), title)
        }

        for (copy in listOf("Copy the join token", "Copy step 2", "Copy step 3")) {
            rule.onNodeWithContentDescription(copy).assertExists()
        }

        assertTrue(present("MYCELIUM_JOIN_TOKEN=myc_join_example mycelium node outpost lab-cluster --namespace team-a --storage-class fast --control-plane https://mycelium.test | kubectl apply -f -"))
        assertEquals(false, present("Copy the join command"))
    }

    @Test
    fun changingTheNameDiscardsTheJoinTokenBecauseItIsBoundToTheName() {
        fillInAnOutpost()
        rule.onNodeWithContentDescription("Create join token").performClick()
        rule.waitUntil(5_000) { present("myc_join_example") }

        rule.onNodeWithContentDescription("Name").performTextInput("2")
        rule.waitForIdle()

        assertEquals(false, present("myc_join_example"))
        rule.onNodeWithContentDescription("Create join token").assertExists()
    }

    @Test
    fun aNameThatIsNotAValidKubernetesNameShowsHowToFixItAndNoCommands() {
        openAddAnOutpost()
        rule.onNodeWithContentDescription("Name").performTextInput("Lab Cluster")
        rule.waitForIdle()

        assertTrue(rule.onAllNodesWithContentDescription("Name. Error: Enter a name of up to 40 lowercase letters, numbers and hyphens, starting and ending with a letter or number.").fetchSemanticsNodes().isNotEmpty())
        assertEquals(false, present("Create the outpost in the cluster"))
    }

    @Test
    fun theNodeListShowsEachOutpostAsItsOwnNodeWithItsNamespace() {
        val first = Cluster.trim().removePrefix("[").removeSuffix("]")
        val second = first.replace("\"id\":\"k1\"", "\"id\":\"k2\"").replace("lab-cluster", "edge-cluster").replace("team-a", "team-b")
        val store = store("""{"kubernetes_nodes":true}""", "[$first,$second]")
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NodesScreen(store, state, KubernetesPlatform) } }
        rule.waitForIdle()

        for (text in listOf("lab-cluster", "edge-cluster", "team-a", "team-b")) {
            assertTrue(present(text), text)
        }

        assertEquals(2, rule.onAllNodesWithText("Kubernetes").fetchSemanticsNodes().size)
    }

    @Test
    fun aKubernetesNodePageShowsTheClusterSettingsAndEachSessionsPod() {
        val store = store("""{"kubernetes_nodes":true}""", Cluster)
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NodePage(store, state, "k1", KubernetesPlatform, onBack = {}, onOpenConversation = {}) } }
        rule.waitForIdle()

        for (text in listOf("Kubernetes", "Outpost", "lab-cluster", "Namespace", "team-a", "Session image", "ghcr.io/mycelium-app/mycelium-agent:latest", "Agent sign-ins", "Secret mycelium-agent-credentials, shared by every session of this outpost", "Workspace", "Lost when a pod is removed", "Conversations running here", "mycelium-session-7f3c2a91", "Needs input", "Agent exited, pod kept", "Closing a conversation deletes its pod.")) {
            assertTrue(present(text), text)
        }

        rule.onNodeWithContentDescription("Open Uploads survive a 503 retry").assertExists()
        assertEquals(false, present("Ports"))
    }

    @Test
    fun aKubernetesNodePageOffersTheSameMoreMenuAsEveryNode() {
        val store = store("""{"kubernetes_nodes":true}""", Cluster)
        rule.setContent { MyceliumTheme(true) { val state by store.state.collectAsState(); NodePage(store, state, "k1", KubernetesPlatform, onBack = {}, onOpenConversation = {}) } }

        rule.onNodeWithContentDescription("More for lab-cluster").performClick()
        rule.waitForIdle()

        for (item in listOf("Rename", "Drain", "Revoke")) {
            assertTrue(present(item), item)
        }
    }
}
