package mycelium.desktop

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
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
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private class ViewerPlatform : Platform {
    val loaded = java.util.Collections.synchronizedList(mutableListOf<String>())

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

    @Composable
    override fun WebContent(url: String, token: String?, modifier: Modifier, reading: mycelium.PageReading?) {
        LaunchedEffect(url) { loaded += url }
    }
}

private fun conversation(id: String) = """{"id":"$id","owner":"u1","node_id":"n1","agent":"claude","cwd":"/work","title":"$id","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private fun artefact(id: String, title: String, conversationId: String?, version: Int, kind: String = "page") =
    """{"id":"$id","owner":"u1","conversation_id":${conversationId?.let { "\"$it\"" } ?: "null"},"kind":"$kind","title":"$title","slug":"$id-ab12","public":false,"latest_version":$version,
"thumbnail_version":null,"url":null,"created_at":"2026-10-22T09:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

class ArtefactViewerTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = ViewerPlatform()
    private var version = 1

    private fun show(route: Route) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/content") -> "# Rendered heading\n\nBody text."
                path.endsWith("/artifacts") -> "[" + artefact("p1", "Planning board", "c1", version) + "," + artefact("p2", "Elsewhere", "c2", 1) + "," + artefact("m1", "Release notes", "c2", 1, "markdown") + "]"
                path.endsWith("/conversations") -> "[" + conversation("c1") + "]"
                path.endsWith("/conversations/c1") -> conversation("c1")
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        refresh = { runBlocking { store.refreshArtifacts() } }

        rule.setContent {
            MyceliumTheme(true) {
                Shell(store, platform, Me("u1", "person@example.test", "Person"), onSignOut = {}, initialRoute = route)
            }
        }
    }

    private var refresh: () -> Unit = {}

    private fun shown(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun composerShown() = rule.onAllNodesWithContentDescription("Message").fetchSemanticsNodes().isNotEmpty()

    @Test
    fun theArtefactsPanelListsThisConversationsArtefactsAndOpensOneBesideTheChat() {
        show(Route.Chat("c1"))

        rule.onNodeWithContentDescription("Open artefacts").performClick()
        rule.waitUntil(5_000) { shown("Planning board") }
        rule.onNodeWithContentDescription("View Planning board").performClick()

        rule.onNodeWithContentDescription("Open full view").assertExists()
        rule.onNodeWithContentDescription("Open in browser").assertExists()
        assertTrue(composerShown())
        assertTrue(!shown("Elsewhere"))
    }

    @Test
    fun clickingACardOnTheArtifactsScreenOpensTheFullViewWithTheAttachedChat() {
        show(Route.Artifacts)

        rule.waitUntil(5_000) { shown("Planning board") }
        rule.onNodeWithContentDescription("Open Planning board").performClick()

        rule.onNodeWithContentDescription("Hide chat").assertExists()
        assertTrue(composerShown())
        assertEquals(listOf("https://mycelium.test/api/v1/artifacts/p1/versions/1/content"), platform.loaded.toList())
    }

    @Test
    fun aNewLatestVersionReloadsTheViewer() {
        show(Route.Artifacts)

        rule.waitUntil(5_000) { shown("Planning board") }
        rule.onNodeWithContentDescription("Open Planning board").performClick()
        rule.waitUntil(5_000) { platform.loaded.isNotEmpty() }

        version = 2
        refresh()

        rule.waitUntil(5_000) { platform.loaded.contains("https://mycelium.test/api/v1/artifacts/p1/versions/2/content") }
    }

    @Test
    fun aMarkdownArtefactIsDrawnNativelyWithoutTheWebView() {
        show(Route.Artifacts)

        rule.waitUntil(5_000) { shown("Release notes") }
        rule.onNodeWithContentDescription("Open Release notes").performClick()

        rule.waitUntil(5_000) { shown("Rendered heading") }
        assertTrue(platform.loaded.isEmpty())
    }
}
