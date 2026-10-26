package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onChildren
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.ui.LocalPlatform
import mycelium.Platform
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.ConversationScreen
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals

private class PreferencesPlatform : Platform {
    val preferences = mutableMapOf<String, String>()
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
    override fun loadPreference(key: String): String? = preferences[key]
    override fun savePreference(key: String, value: String) {
        preferences[key] = value
    }
}

private fun conversation(id: String, cwd: String) = """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"$cwd","title":"$id","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private val Conversations = mapOf("a1" to "/work/a", "b1" to "/work/b", "a2" to "/work/a")

class SidePanelMemoryTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = PreferencesPlatform()
    private var shown by mutableStateOf("a1")

    private fun show() {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val id = Conversations.keys.firstOrNull { path.endsWith("/conversations/$it") }
            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/conversations") -> Conversations.entries.joinToString(",", "[", "]") { conversation(it.key, it.value) }
                id != null -> conversation(id, Conversations.getValue(id))
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent {
            CompositionLocalProvider(LocalPlatform provides platform) {
                MyceliumTheme(true) {
                    val state by store.state.collectAsState()

                    ConversationScreen(store, state, shown, onArchived = {})
                }
            }
        }
    }

    @Test
    fun eachConversationReopensTheSidePanelItHadOpen() {
        show()
        rule.onNodeWithContentDescription("Open details").performClick()

        shown = "b1"
        rule.waitForIdle()
        rule.onNodeWithContentDescription("Open details").assertExists()

        shown = "a1"
        rule.waitForIdle()
        rule.onNodeWithContentDescription("Close details").assertExists()
    }

    @Test
    fun aResizedSidePanelKeepsItsWidthInItsOwnWorkspaceOnly() {
        show()
        rule.onNodeWithContentDescription("Open details").performClick()
        val handle = rule.onNodeWithContentDescription("Resize the side panel")
        val before = handle.getBoundsInRoot().left
        handle.onChildren().onFirst().requestFocus()
        handle.onChildren().onFirst().performKeyInput { pressKey(Key.DirectionLeft) }
        rule.waitForIdle()
        val widened = handle.getBoundsInRoot().left

        shown = "b1"
        rule.waitForIdle()
        rule.onNodeWithContentDescription("Open details").performClick()
        val other = rule.onNodeWithContentDescription("Resize the side panel")
        other.onChildren().onFirst().requestFocus()
        other.onChildren().onFirst().performKeyInput { pressKey(Key.DirectionRight); pressKey(Key.DirectionRight) }
        rule.waitForIdle()
        val elsewhere = other.getBoundsInRoot().left

        shown = "a2"
        rule.waitForIdle()
        rule.onNodeWithContentDescription("Open details").performClick()
        val back = rule.onNodeWithContentDescription("Resize the side panel").getBoundsInRoot().left

        assertNotEquals(before, widened)
        assertNotEquals(widened, elsewhere)
        assertEquals(widened, back)
    }
}
