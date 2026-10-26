package mycelium.desktop

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.background
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.ActivityRow
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.ui.Col
import mycelium.ui.DataTable
import mycelium.ui.ChatToolsSettings
import mycelium.ui.IntegrationPage
import mycelium.ui.IntegrationUsage
import mycelium.ui.PageActions
import mycelium.ui.integrationsOf
import mycelium.ui.HistoryGroup
import mycelium.ui.HistoryEntry
import mycelium.ui.HistoryGrouping
import mycelium.ui.HistoryLayout
import mycelium.ui.HistoryTable
import mycelium.ui.RowMenu
import mycelium.ui.ForgesSettings
import mycelium.ui.LocalTouch
import mycelium.ui.ModelProvidersSettings
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.Overflow
import mycelium.ui.SessionsSettings
import mycelium.ui.Table
import mycelium.ui.TableCell
import mycelium.ui.TableColumn
import mycelium.ui.ThreadView
import org.jetbrains.skia.EncodedImageFormat
import org.junit.Rule
import java.io.File
import kotlin.test.Test
import kotlin.test.assertTrue

private const val Laptop = "aaaaaaaa-0000-0000-0000-000000000001"

private fun archived(id: String, cwd: String, title: String, updated: String) =
    """{"id":"$id","owner":"u","node_id":"$Laptop","agent":"claude","cwd":"$cwd","title":"$title","status":"done","acp_session_id":null,"mode":null,"model":null,"created_at":"$updated","updated_at":"$updated","archived_at":"$updated"}"""

private val RenderNodes = """[{"id":"$Laptop","name":"laptop-with-a-long-name","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","address":"100.64.4.18","online":true,"draining":false,"agents":[],"logins":[]}]"""

private val RenderArchived = "[" + listOf(
    archived("c1", "/home/sam/projects/okapi", "Plan the rollout of the new sync engine across every node and check the logs afterwards", "2026-10-22T10:00:00Z"),
    archived("c2", "/home/sam/projects/pangolin", "Audit logs", "2026-10-21T10:00:00Z"),
    archived("c3", "/home/sam/projects/okapi", "Zebra fixes", "2026-10-20T10:00:00Z"),
).joinToString(",") + "]"

private const val RenderTools = """[{"id":"s1","name":"Lab","url":"https://lab.test/mcp","token_hint":"cdef","enabled":true,"always_allow":false,"tools":[{"name":"get_weather","description":"Get the weather"},{"name":"list_files","description":"List files and a very long description that should not break the table layout"}],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val RenderForges = """[{"id":"f1","kind":"github","base_url":"https://github.com","account":"samexample","token_hint":"wxyz","organisations":["samexample","acme-org","immich-app","another-organisation"],"created_at":"2026-10-22T10:00:00Z"},
{"id":"f2","kind":"gitlab","base_url":"https://gitlab.example.com","account":"sam","token_hint":"1234","organisations":["sam","slop"],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val RenderSessions = """[
{"id":"s1","kind":"browser","label":"Firefox on Linux","hostname":"","address":"100.64.4.18","last_used_at":"2026-10-23T10:00:00Z","current":true},
{"id":"s2","kind":"client","label":"Desktop app","hostname":"willow-laptop","address":"100.64.4.18","last_used_at":"2026-10-23T09:00:00Z","current":false}]"""

private const val RenderProviders = """[{"id":"p1","kind":"openai_compatible","name":"Ollama on the workstation","base_url":"http://workstation.tailnet.example:11434/v1","models":["llama3.2","qwen3"],"created_at":"2026-10-22T10:00:00Z"},
{"id":"p2","kind":"anthropic","name":"Claude API","key_hint":"wxyz","models":["claude-sonnet-5-5","claude-opus-5-5"],"default_model":"claude-sonnet-5-5","created_at":"2026-10-22T10:00:00Z"}]"""

private const val Markdown = "Here is the outcome.\n\n| File | Resolution | Lines |\n|---|---|--:|\n| restore.svelte | Keeps the outcome and progress version with no log observer, plus the new finish rule for complete or warn and the cancelled or failed title, and errors come from the outcome. | 12 |\n| api.ts | Short | 1204 |"

class TablesRenderTest {
    @get:Rule
    val scopes = TestScopes()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            val body = when {
                path.endsWith("/nodes") -> RenderNodes
                path.endsWith("/chat-tool-servers") -> RenderTools
                path.endsWith("/forges") -> RenderForges
                path.endsWith("/sessions") -> RenderSessions
                path.endsWith("/model-providers") -> RenderProviders
                path.endsWith("/conversations") && request.url.parameters["archived"] == "true" -> RenderArchived
                else -> "[]"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        return store
    }

    @OptIn(InternalComposeUiApi::class)
    private fun renderWidth(name: String, dark: Boolean, width: Int, height: Int = 700, content: @Composable () -> Unit) {
        val scene = ImageComposeScene(width, height, Density(1f)) {
            MyceliumTheme(dark) {
                CompositionLocalProvider(LocalTouch provides (width < 480)) {
                    Column(Modifier.fillMaxSize().background(MyTheme.colors.bgContent).padding(if (width < 480) 8.dp else 16.dp)) { content() }
                }
            }
        }

        var image = scene.render(0)

        for (frame in 1..40) {
            Thread.sleep(50)
            image = scene.render(frame * 16_000_000L)
        }

        val out = File("build/screens/tables-$name-$width-${if (dark) "dark" else "light"}.png")
        out.parentFile.mkdirs()
        out.writeBytes(image.encodeToData(EncodedImageFormat.PNG)!!.bytes)
        scene.close()

        assertTrue(out.length() > 2_000, name)
    }

    private fun everywhere(name: String, height: Int = 700, content: @Composable () -> Unit) {
        for (width in listOf(320, 900)) {
            for (dark in listOf(true, false)) {
                renderWidth(name, dark, width, height, content)
            }
        }
    }

    @Test
    fun historyRenders() {
        val entries = listOf(
            Triple("Plan the rollout of the new sync engine across every node and check the logs afterwards", "okapi", 1204L),
            Triple("Audit logs", "pangolin", 88L),
            Triple("Zebra fixes", "okapi", 5L),
        ).mapIndexed { index, (title, project, tokens) ->
            val conversation = Conversation("c$index", "u", Laptop, "claude", "/home/sam/projects/$project", title, ConversationStatus.Done, createdAt = "2026-10-22T10:00:00Z", updatedAt = "2026-10-22T10:00:00Z")

            HistoryEntry(conversation, project, "laptop-with-a-long-name", ActivityRow("c$index", title, turns = 3L + index, reportedTurns = 3L, inputTokens = tokens * 1000, outputTokens = tokens, costUsd = 1.5 * index))
        }

        for (grouping in listOf(HistoryGrouping.None, HistoryGrouping.Project)) {
            val groups = if (grouping == HistoryGrouping.None) listOf(HistoryGroup("all", "All", entries)) else entries.groupBy { it.project }.map { (project, rows) -> HistoryGroup(project, project, rows) }

            everywhere("history-${grouping.name.lowercase()}", 400) {
                HistoryTable(groups, HistoryLayout(grouping = grouping), {}, emptyMap(), {}, { iso, _ -> iso.take(10) }, {}) { RowMenu(it.title) { } }
            }
        }
    }

    @Test
    fun integrationsRender() {
        val store = store()
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val api = MyceliumApi("https://mycelium.test", "token", MockEngine { respond(RenderTools, HttpStatusCode.OK, json) })
        val integration = integrationsOf(runBlocking { api.chatToolServers() }).first()
        val actions = PageActions({}, {}, {}, {}, {}, {})

        everywhere("integrations-home", 700) { ChatToolsSettings(store) }
        everywhere("integration-page", 900) { IntegrationPage(store, integration, actions, {}) }
        everywhere("integration-usage", 700) { IntegrationUsage(store, integration, {}, {}) }
    }

    @Test
    fun forgesRender() {
        val store = store()

        everywhere("forges", 400) { ForgesSettings(store) }
    }

    @Test
    fun sessionsAndNodesRender() {
        val store = store()

        everywhere("sessions", 600) { SessionsSettings(store, onSignOut = {}) }
    }

    @Test
    fun chatModelsRender() {
        val store = store()

        everywhere("models", 500) { ModelProvidersSettings(store) }
    }

    @Test
    fun markdownTableRenders() {
        everywhere("markdown", 400) { ThreadView(listOf(ThreadItem.AgentMessage("m1", Markdown)), onAnswer = { _, _ -> }, onStopWorkflow = {}) }
    }

    @Test
    fun dataTableRenders() {
        everywhere("data", 400) {
            DataTable(
                "Busiest processes",
                listOf(TableColumn("Process", 2f), TableColumn("CPU", numeric = true), TableColumn("Memory", numeric = true)),
                listOf(listOf("/usr/lib/firefox/firefox-bin --type=renderer", "12%", "1.4 GB"), listOf("mycelium-node", "3%", "88 MB")),
                "No processes reported.",
            )
        }
    }

    @Test
    fun fixtureTableRenders() {
        everywhere("fixture", 400) {
            Table(
                listOf(Col("Name", 120.dp, weight = 1f), Col("Path", 120.dp, max = 300.dp), Col("Count", 56.dp, numeric = true), Col("Status", 72.dp), Col("Notes", 100.dp, overflow = Overflow.Wrap, priority = 1)),
                3,
                { row, column ->
                    TableCell.Text(
                        listOf(
                            listOf("A title that runs on and on and on well past any column in the table", "/home/claude/projects/mycelium/clients/shared/src", "5", "", "Notes long enough to wrap onto a second line in a narrow wrapping column"),
                            listOf("Short", "/tmp", "12345", "Ready", "ok"),
                            listOf("Third", "/var", "7", "Idle", "fine"),
                        )[row][column],
                    )
                },
            )
        }
    }
}
