package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.BrowserPanel
import mycelium.ui.IdesSettings
import mycelium.ui.MyTheme
import mycelium.ui.ButtonVariant
import mycelium.ui.MyceliumTheme
import mycelium.ui.NewConversationScreen
import mycelium.ui.SplitAlternative
import mycelium.ui.SplitButton
import mycelium.ui.ButtonSize
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private const val NodeJson = """[{"id":"n1","name":"atlas-box","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[],
"harness_models":[{"agent":"claude","models":[{"id":"opus","name":"Opus 5.5"},{"id":"sonnet","name":"Sonnet 5.5"}],"default_model":"opus","reported_at":"2026-10-22T08:00:00Z"}]}]"""

private const val ProjectJson = """[{"id":"p1","node_id":"n1","name":"harbour","path":"/home/you/harbour","created_at":"2026-10-22T10:00:00Z"}]"""

@OptIn(androidx.compose.ui.InternalComposeUiApi::class)
class ConversationAreaRenderTest {
    @get:Rule
    val scopes = TestScopes()

    @Test
    fun newConversationWithANewBranchAndTheIdesAndOutlinePanelScreens() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/projects") -> respond(ProjectJson, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(NodeJson, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.refreshProjects() }

        val ides = renderAt("settings-ides", true, false) {
            Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent).padding(24.dp)) { IdesSettings(PlatformStub) }
        }
        val outline = renderAt("outline-panel", true, false) {
            Box(Modifier.width(480.dp).fillMaxSize().background(MyTheme.colors.bgContent)) {
                BrowserPanel(PlatformStub, "https://docs.example.org/doc/harbour-release-notes-v040", "harbour release notes, v0.4.0", onClose = {})
            }
        }

        assertTrue(ides.length() > 1_000 && outline.length() > 1_000)
    }
}

class ConversationAreaInteractionTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun store(): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/projects") -> respond(ProjectJson, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond(NodeJson, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh(); store.refreshProjects() }

        return store
    }

    private fun capture(name: String) {
        val png = org.jetbrains.skia.Image.makeFromBitmap(rule.onAllNodes(isRoot()).onLast().captureToImage().asSkiaBitmap()).encodeToData(org.jetbrains.skia.EncodedImageFormat.PNG)!!.bytes
        val out = java.io.File("build/screens/$name-dark.png")
        out.parentFile.mkdirs()
        out.writeBytes(png)
    }

    private fun showNewConversation() {
        val store = store()

        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(900.dp).fillMaxSize().background(MyTheme.colors.bgContent)) {
                    val state by store.state.collectAsState()

                    NewConversationScreen(store, state, onCreated = {}, projectId = "p1")
                }
            }
        }
    }

    @Test
    fun aNewWorktreeNamesItsFolderAndTheBranchItStartsFrom() {
        showNewConversation()

        rule.onNodeWithContentDescription("Place: ", substring = true).performClick()
        rule.onNodeWithText("New worktree").performClick()
        rule.onNodeWithContentDescription("New branch", substring = true).performTextInput("fix/restore-copy")

        rule.onNodeWithText("Folder harbour-worktrees/fix-restore-copy", substring = true).assertExists()
        capture("new-conversation-branch")
    }

    @Test
    fun theModelMenuOffersTheDefaultAndTheListTheNodeReported() {
        showNewConversation()

        rule.onNodeWithContentDescription("Model", substring = true).performClick()

        assertTrue(rule.onAllNodesWithText("Opus 5.5").fetchSemanticsNodes().isNotEmpty())
        rule.onNodeWithText("Sonnet 5.5").assertExists()
        capture("new-conversation-model-menu")
    }
}
