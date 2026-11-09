package mycelium.desktop

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
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
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private class RowsPlatform : Platform {
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
    override fun loadPreference(key: String): String? = null
    override fun savePreference(key: String, value: String) = Unit
}

private fun conversation(id: String, title: String, status: String) = """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/$id","title":"$title","status":"$status","project_id":"p1",
"acp_session_id":null,"mode":null,"model":null,"group_id":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null,"pinned_at":null}"""

private fun unread(row: String) = row.dropLast(1) + ""","unread":true}"""

private fun hasContentDescriptionStarting(prefix: String) = SemanticsMatcher("description starts with $prefix") { node ->
    node.config.getOrNull(SemanticsProperties.ContentDescription)?.any { it.startsWith(prefix) } == true
}

private const val ProjectsJson = """[{"id":"p1","node_id":"n1","name":"Pipeline","path":"/work/pipeline","icon":"folder","created_at":"2026-10-22T10:00:00Z"}]"""

private const val NodesJson = """[{"id":"n1","name":"Workbench","hostname":"bench","os":"linux","arch":"x86_64","version":"1.0.0","online":true,"draining":false,"sessions":["w1","w2","w3"]}]"""

class ConversationRowStatusTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val opened = java.util.concurrent.CopyOnWriteArrayList<String>()

    private fun show(rows: List<String>, assistants: String = "[]") {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (path.endsWith("/opened")) {
                opened += path
            }

            val body = when {
                path.endsWith("/conversations") -> "[${rows.joinToString(",")}]"
                path.endsWith("/projects") -> ProjectsJson
                path.endsWith("/nodes") -> NodesJson
                path.endsWith("/assistants") -> assistants
                path.endsWith("/groups") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, RowsPlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}) } }
    }

    @Test
    fun eachRowNamesItsStateAgentAndNodeInWords() {
        val rows = listOf(conversation("w1", "Build the thing", "working"), conversation("w2", "Pick a colour", "input"), conversation("w3", "Break the build", "failed"))
        show(rows)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Pipeline Build the thing").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Pipeline Build the thing, Working, Claude Code on Workbench").assertExists()
        rule.onNodeWithContentDescription("Pipeline Pick a colour, Needs input, Claude Code on Workbench").assertExists()
        rule.onNodeWithContentDescription("Pipeline Break the build, Failed, Claude Code on Workbench").assertExists()
    }

    @Test
    fun anUnreadRowIsNamedUnreadAndARowAlreadyReadIsNot() {
        show(listOf(unread(conversation("w1", "Fresh reply", "review")), conversation("w2", "Seen reply", "review"), unread(conversation("w3", "Fresh question", "input"))))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Pipeline Fresh reply").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Pipeline Fresh reply, unread, Ready to review, Claude Code on Workbench").assertExists()
        rule.onNodeWithContentDescription("Pipeline Seen reply, Ready to review, Claude Code on Workbench").assertExists()
        rule.onNodeWithContentDescription("Pipeline Fresh question, unread, Needs input, Claude Code on Workbench").assertExists()
        rule.captureScreen("sidebar-unread")
    }

    @Test
    fun openingAnUnreadRowTellsTheServerItHasBeenRead() {
        show(listOf(unread(conversation("w2", "Fresh question", "input"))))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Pipeline Fresh question").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(emptyList(), opened.toList())

        rule.onNodeWithContentDescription("Pipeline Fresh question, unread, Needs input, Claude Code on Workbench").performClick()

        rule.waitUntil(5_000) { opened.toList() == listOf("/api/v1/conversations/w2/opened") }
    }

    @Test
    fun anAssistantWhoseThreadHasAnUnreadReplyIsNamedUnread() {
        val assistants = """[{"id":"as1","name":"Archivist","group_id":null,"preferences":{"models":[],"nodes":[],"chat_first":true},"conversation_id":"w1","state":"idle","created_at":"2026-10-22T10:00:00Z"},{"id":"as2","name":"Release captain","group_id":null,"preferences":{"models":[],"nodes":[],"chat_first":true},"conversation_id":"w2","state":"idle","created_at":"2026-10-22T10:00:00Z"}]"""
        show(listOf(unread(conversation("w1", "Archivist thread", "review")), conversation("w2", "Captain thread", "review")), assistants)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Archivist").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(1, rule.onAllNodes(hasContentDescriptionStarting("Archivist, unread, ")).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(hasContentDescriptionStarting("Release captain, unread, ")).fetchSemanticsNodes().size)
    }

    @Test
    fun aStandaloneMainBranchIsHiddenWhileFeatureBranchesAndPullRequestsRemain() {
        val main = conversation("w1", "Main work", "working").dropLast(1) + ""","branch":"main"}"""
        val feature = conversation("w2", "Feature work", "input").dropLast(1) + ""","branch":"topic","pull_request_label":"PR #7","pull_request_state":"open"}"""
        val mainPull = conversation("w3", "Review work", "failed").dropLast(1) + ""","branch":"main","pull_request_label":"PR #8","pull_request_state":"open"}"""
        show(listOf(main, feature, mainPull))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Pipeline Main work").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("main").fetchSemanticsNodes().size)
        rule.onNodeWithText("topic  PR #7 open").assertExists()
        rule.onNodeWithText("main  PR #8 open").assertExists()
        rule.captureScreen("sidebar-compact-branches")
    }
}
