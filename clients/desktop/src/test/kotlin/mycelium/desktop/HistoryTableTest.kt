package mycelium.desktop

import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Conversation
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.StreamMessage
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val LaptopId = "aaaaaaaa-0000-0000-0000-000000000001"

private const val ServerId = "aaaaaaaa-0000-0000-0000-000000000002"

private const val TwoNodesJson = """[
{"id":"$LaptopId","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]},
{"id":"$ServerId","name":"server","hostname":"server","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]}]"""

private fun archivedJson(id: String, node: String, cwd: String, title: String, updated: String) =
    """{"id":"$id","owner":"u","node_id":"$node","agent":"claude","cwd":"$cwd","title":"$title","status":"done","acp_session_id":null,"mode":null,"model":null,"created_at":"$updated","updated_at":"$updated","archived_at":"$updated"}"""

private val ThreeArchivedJson = "[" + listOf(
    archivedJson("c1", LaptopId, "/home/sam/projects/okapi", "Plan the rollout", "2026-10-22T10:00:00Z"),
    archivedJson("c2", ServerId, "/home/sam/projects/pangolin", "Audit logs", "2026-10-21T10:00:00Z"),
    archivedJson("c3", ServerId, "/home/sam/projects/okapi", "Zebra fixes", "2026-10-20T10:00:00Z"),
).joinToString(",") + "]"

private object HistoryTablePlatform : Platform {
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

class HistoryTableTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/nodes") -> TwoNodesJson
                path.endsWith("/activity/conversations") -> (1..4).joinToString(",", "[", "]") { """{"key":"c$it","name":"","turns":1}""" }
                path.endsWith("/conversations") && request.url.parameters["archived"] == "true" -> ThreeArchivedJson
                path.endsWith("/conversations") -> "[]"
                else -> "[]"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        return store
    }

    private fun showHistory(): Store {
        val store = store()
        rule.setContent { MyceliumTheme(true) { Shell(store, HistoryTablePlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Archive) } }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open Zebra fixes").fetchSemanticsNodes().isNotEmpty() }

        return store
    }

    private fun rowTitles(): List<String> = listOf("Plan the rollout", "Audit logs", "Zebra fixes")
        .mapNotNull { title -> rule.onAllNodesWithContentDescription("Open $title").fetchSemanticsNodes().firstOrNull()?.let { title to it.positionInRoot.y } }
        .sortedBy { it.second }
        .map { it.first }

    private fun pickMenuItem(button: String, item: String) {
        rule.onNodeWithContentDescription(button).performClick()
        rule.onAllNodes(hasText(item) and hasClickAction()).let { nodes -> nodes[nodes.fetchSemanticsNodes().size - 1] }.performClick()
    }

    @Test
    fun rowsAlreadyShownStayInLineWithANewRowThatWidensAColumn() {
        val store = showHistory()
        val widening = MyceliumJson.decodeFromString(Conversation.serializer(), archivedJson("c4", LaptopId, "/home/sam/projects/a-project-with-a-far-longer-name-than-the-rest", "Newest", "2026-10-23T10:00:00Z").replace(""""archived_at":"2026-10-23T10:00:00Z"""", """"archived_at":null"""))

        store.receive(StreamMessage.ConversationChanged(widening))
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open Newest").fetchSemanticsNodes().isNotEmpty() }
        rule.waitForIdle()

        val newest = rule.onNodeWithText("a-project-with-a-far-longer-name-than-the-rest").fetchSemanticsNode().positionInRoot.x
        val older = rule.onAllNodesWithText("okapi").fetchSemanticsNodes().map { it.positionInRoot.x }

        assertEquals(listOf(newest, newest), older)
    }

    @Test
    fun clickingAColumnHeaderSortsAscendingThenDescending() {
        showHistory()
        assertEquals(listOf("Plan the rollout", "Audit logs", "Zebra fixes"), rowTitles())

        rule.onNodeWithContentDescription("Sort by Conversation").performClick()
        rule.waitForIdle()
        assertEquals(listOf("Audit logs", "Plan the rollout", "Zebra fixes"), rowTitles())

        rule.onNodeWithContentDescription("Sort by Conversation").performClick()
        rule.waitForIdle()
        assertEquals(listOf("Zebra fixes", "Plan the rollout", "Audit logs"), rowTitles())
    }

    @Test
    fun filteringByNodeHidesConversationsFromOtherNodes() {
        showHistory()

        pickMenuItem("Filter by node", "laptop")
        rule.waitForIdle()

        assertEquals(listOf("Plan the rollout"), rowTitles())
    }

    @Test
    fun groupingByProjectGivesEachProjectACollapsibleHeader() {
        showHistory()

        pickMenuItem("Group conversations", "By project")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Collapse okapi").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Collapse pangolin").assertExists()

        rule.onNodeWithContentDescription("Collapse okapi").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open Plan the rollout").fetchSemanticsNodes().isEmpty() }
        rule.onNodeWithContentDescription("Expand okapi").assertExists()
        rule.onNodeWithContentDescription("Open Audit logs").assertExists()
    }

    @Test
    fun aSavedViewReappliesItsGroupingAfterTheLayoutChanges() {
        showHistory()

        pickMenuItem("Group conversations", "By node")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Collapse server").fetchSemanticsNodes().isNotEmpty() }

        pickMenuItem("Views", "Save view")
        rule.onNodeWithContentDescription("View name").performTextInput("By node")
        rule.onAllNodes(hasText("Save view") and hasClickAction()).let { nodes -> nodes[nodes.fetchSemanticsNodes().size - 1] }.performClick()

        pickMenuItem("Group conversations", "No grouping")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Collapse server").fetchSemanticsNodes().isEmpty() }

        pickMenuItem("Views", "By node")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Collapse server").fetchSemanticsNodes().isNotEmpty() }
    }
}
