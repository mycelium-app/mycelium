package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
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
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Providers = """[{"id":"p1","kind":"openai_compatible","name":"Ollama","models":["llama3.2","qwen"],"default_model":"llama3.2","key_hint":"","created_at":"2026-10-22T10:00:00Z"}]"""

private const val Saved = """{"id":"a1","name":"Stand-up","description":"Daily summary","preferences":{"models":[{"provider_id":"p1","model":"llama3.2"},{"provider_id":"p1","model":"qwen"}]},"state":"idle","session_count":0,"instructions":"Be brief.","created_at":"2026-10-22T10:00:00Z"}"""

private const val Nodes = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.11.0","address":"10.0.0.5","online":true,"draining":false,"sessions":[],"agents":[{"agent":"claude","available":true}],"logins":[]}]"""

private const val Forges = """[{"id":"f1","kind":"github","base_url":"https://github.com","account":"samexample","token_hint":"ghp_…","organisations":[],"created_at":"2026-10-22T10:00:00Z"}]"""

private const val Secrets = """[{"name":"GITHUB_TOKEN","description":"Pushes","state":"waiting"}]"""

private const val Files = """[{"path":"AGENTS.md","size":9,"updated_at":"2026-10-22T10:00:00Z"},{"path":"notes/today.md","size":10,"updated_at":"2026-10-22T10:00:00Z"}]"""

private object SettingsPlatform : Platform {
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

class AssistantSettingsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private fun shell() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            requests += "${request.method.value} $path" to String(request.body.toByteArray())

            val reply = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/forges") -> Forges
                path.endsWith("/assistants/a1/backup") && request.method == HttpMethod.Put -> Saved
                path.endsWith("/assistants/a1/secrets") -> Secrets
                path.endsWith("/secrets/GITHUB_TOKEN") && request.method == HttpMethod.Put -> """{"name":"GITHUB_TOKEN","state":"set","set_by":"person"}"""
                path.endsWith("/assistants/a1/files") -> Files
                path.endsWith("/files/notes/today.md") && request.method == HttpMethod.Get -> "Shipped v1"
                path.endsWith("/files/notes/today.md") && request.method == HttpMethod.Put -> """{"path":"notes/today.md","size":10,"updated_at":"2026-10-22T10:00:00Z"}"""
                path.endsWith("/assistants/a1") && request.method == HttpMethod.Patch -> Saved
                path.endsWith("/assistants/a1") -> Saved
                path.endsWith("/assistants") -> "[$Saved]"
                path.endsWith("/model-providers") -> Providers
                path.endsWith("/nodes") -> Nodes
                path.endsWith("/groups") || path.endsWith("/projects") || path.endsWith("/conversations") -> "[]"
                else -> "{}"
            }

            respond(reply, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }

        rule.setContent { MyceliumTheme(true) { Shell(store, SettingsPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.AssistantSettings("a1")) } }
    }

    private fun openSection(label: String) {
        rule.waitUntil(5_000) { rule.onAllNodesWithText(label).fetchSemanticsNodes().isNotEmpty() }
        rule.onAllNodesWithText(label)[0].performClick()
    }

    @Test
    fun theBackButtonLeavesSettingsForTheAssistantsThread() {
        shell()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Back to Stand-up").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Back to Stand-up").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for Stand-up").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun modelPreferencesReorderWithMoveUpAndAreSavedInThatOrder() {
        shell()
        openSection("Models and nodes")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for Ollama · qwen").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for Ollama · qwen").performClick()
        rule.onNodeWithText("Move up").performClick()
        rule.onNodeWithText("Save").performScrollTo().performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PATCH /api/v1/assistants/a1" } }
        val sent = requests.last { it.first == "PATCH /api/v1/assistants/a1" }.second

        assertEquals(true, sent.indexOf("qwen") in 0 until sent.indexOf("llama3.2"), sent)
    }

    @Test
    fun theStorageTableListsFilesApartFromTheInstructionsAndSavesAnEdit() {
        shell()
        openSection("Storage and secrets")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for notes/today.md").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("More for AGENTS.md").fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("More for notes/today.md").performScrollTo().performClick()
        rule.onNodeWithText("Edit").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Edit notes/today.md").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Edit notes/today.md").performScrollTo().performTextInput(" and v2")
        rule.onNodeWithText("Save file").performScrollTo().performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/assistants/a1/files/notes/today.md" } }
        assertEquals(true, requests.last { it.first.startsWith("PUT") }.second.contains("and v2"))
    }

    @Test
    fun addingClaudeCodeAndANodeIsSavedAsPreferences() {
        shell()
        openSection("Models and nodes")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Add a model: Add a model").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Add a model: Add a model").performScrollTo().performClick()
        rule.onNodeWithText("Claude Code, any account").performClick()
        rule.onNodeWithText("Add node").performScrollTo().performClick()
        rule.onNodeWithText("willow").performClick()
        rule.onNodeWithContentDescription("Chat first when no files are needed").performScrollTo().performClick()
        rule.onNodeWithText("Save").performScrollTo().performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PATCH /api/v1/assistants/a1" } }
        val sent = requests.last { it.first == "PATCH /api/v1/assistants/a1" }.second

        assertEquals(true, """{"agent":"claude"}""" in sent && """"nodes":["n1"]""" in sent && """"chat_first":false""" in sent, sent)
    }

    @Test
    fun aSecretIsEditedInAModalWithoutShowingTheValue() {
        shell()
        openSection("Storage and secrets")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for GITHUB_TOKEN").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for GITHUB_TOKEN").performScrollTo().performClick()
        rule.onNodeWithText("Edit").performClick()
        rule.onNodeWithContentDescription("What it is for").performTextClearance()
        rule.onNodeWithContentDescription("What it is for").performTextInput("Pushes to the repository")
        rule.onNodeWithContentDescription("New value, leave empty to keep the current one").performTextInput("ghp_abcdef")
        rule.onNodeWithText("Save secret").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/assistants/a1/secrets/GITHUB_TOKEN" } }
        assertEquals("""{"value":"ghp_abcdef","description":"Pushes to the repository"}""", requests.last { it.first.startsWith("PUT") }.second)
        rule.waitUntil(5_000) { rule.onAllNodesWithText("ghp_abcdef").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun aSecretsDescriptionChangesWithoutSendingAValue() {
        shell()
        openSection("Storage and secrets")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("More for GITHUB_TOKEN").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("More for GITHUB_TOKEN").performScrollTo().performClick()
        rule.onNodeWithText("Edit").performClick()
        rule.onNodeWithContentDescription("What it is for").performTextClearance()
        rule.onNodeWithContentDescription("What it is for").performTextInput("Pushes releases")
        rule.onNodeWithText("Save secret").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/assistants/a1/secrets/GITHUB_TOKEN" } }
        assertEquals("""{"description":"Pushes releases"}""", requests.last { it.first.startsWith("PUT") }.second)
    }

    @Test
    fun aNewSecretTakesItsNameDescriptionAndValueInOneModal() {
        shell()
        openSection("Storage and secrets")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Add secret").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Add secret").performScrollTo().performClick()
        rule.onNodeWithContentDescription("Name").performTextInput("NPM_TOKEN")
        rule.onNodeWithContentDescription("What it is for").performTextInput("Publishes packages")
        rule.onNodeWithContentDescription("Value").performTextInput("npm_abcdef")
        rule.onNodeWithText("Save secret").performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/assistants/a1/secrets/NPM_TOKEN" } }
        assertEquals("""{"value":"npm_abcdef","description":"Publishes packages"}""", requests.last { it.first.startsWith("PUT") }.second)
    }

    @Test
    fun aRepositoryIsChosenForTheBackup() {
        shell()
        openSection("Storage and secrets")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Repository").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Repository").performScrollTo().performTextInput("samexample/standup")
        rule.onNodeWithText("Back up to this repository").performScrollTo().performClick()

        rule.waitUntil(5_000) { requests.any { it.first == "PUT /api/v1/assistants/a1/backup" } }
        assertEquals("""{"forge_id":"f1","repository":"samexample/standup"}""", requests.last { it.first == "PUT /api/v1/assistants/a1/backup" }.second)
    }
}
