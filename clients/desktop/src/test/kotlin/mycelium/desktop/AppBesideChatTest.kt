package mycelium.desktop

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
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
import mycelium.PageReading
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

private class BesidePlatform(private val inner: AppsPlatform) : Platform by inner {
    val shown = java.util.Collections.synchronizedList(mutableListOf<String>())
    val preferences = java.util.concurrent.ConcurrentHashMap<String, String>()

    override fun loadPreference(key: String): String? = preferences[key]

    override fun savePreference(key: String, value: String) {
        preferences[key] = value
    }

    @Composable
    override fun WebContent(url: String, token: String?, modifier: Modifier, reading: PageReading?) {
        LaunchedEffect(url) { shown += url }
    }
}

private const val BuiltInChat = """{"uuid":"a1","name":"harbour-status","status":"running","address":"https://harbour-status.apps.example.com","conversation_id":"c1"}"""

private const val BuiltOutside = """{"uuid":"a2","name":"orphan-app","status":"running","address":"https://orphan-app.apps.example.com"}"""

private const val ConversationJson = """{"id":"c1","owner":"u1","node_id":"n1","agent":"claude","cwd":"/work","title":"c1","status":"done",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

class AppBesideChatTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = BesidePlatform(AppsPlatform())

    private fun show(route: Route) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/apps/a1") -> BuiltInChat
                path.endsWith("/apps/a2") -> BuiltOutside
                path.endsWith("/apps") -> """{"apps":[$BuiltInChat,$BuiltOutside]}"""
                path.endsWith("/conversations") -> "[$ConversationJson]"
                path.endsWith("/conversations/c1") -> ConversationJson
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking {
            store.refresh()
            store.refreshApps()
        }

        rule.setContent {
            MyceliumTheme(true) {
                Shell(store, platform, Me("u1", "person@example.test", "Person"), onSignOut = {}, initialRoute = route)
            }
        }
    }

    private fun composerShown() = rule.onAllNodesWithContentDescription("Message").fetchSemanticsNodes().isNotEmpty()

    @Test
    fun anAppBuiltInTheConversationOpensInItsSidePanelBesideTheChat() {
        show(Route.Chat("c1"))

        rule.onNodeWithContentDescription("Open apps").performClick()
        rule.onNodeWithContentDescription("View harbour-status").performClick()

        rule.waitUntil(5_000) { platform.shown.isNotEmpty() }
        assertEquals(listOf("https://harbour-status.apps.example.com"), platform.shown.toList())
        rule.onNodeWithContentDescription("Open in browser").assertExists()
        assertTrue(composerShown())
    }

    @Test
    fun closingTheOpenAppInTheSidePanelReturnsToTheConversationsApps() {
        show(Route.Chat("c1"))

        rule.onNodeWithContentDescription("Open apps").performClick()
        rule.onNodeWithContentDescription("View harbour-status").performClick()
        rule.onNodeWithContentDescription("Close harbour-status").performClick()

        rule.onNodeWithContentDescription("View harbour-status").assertExists()
    }

    @Test
    fun theChatThatBuiltAnOpenAppIsShownBesideItAndCanBeHidden() {
        show(Route.App("a1", viewing = true))

        rule.waitUntil(5_000) { composerShown() }
        assertEquals(listOf("https://harbour-status.apps.example.com"), platform.shown.toList())

        rule.onNodeWithContentDescription("Hide chat").performClick()

        assertTrue(!composerShown())
        rule.onNodeWithContentDescription("Show chat").assertExists()
    }

    @Test
    fun anAppWithNoConversationOffersAPickerAndShowsTheChosenConversationBesideIt() {
        show(Route.App("a2", viewing = true))

        rule.waitUntil(5_000) { platform.shown.isNotEmpty() }
        assertTrue(!composerShown())

        rule.onNodeWithContentDescription("Show chat").performClick()
        rule.onNodeWithContentDescription("Search conversations").assertExists()
        rule.onNodeWithText("c1").performClick()

        rule.waitUntil(5_000) { composerShown() }
        rule.onNodeWithContentDescription("Hide chat").assertExists()
        assertEquals("c1", platform.preferences["beside-chat:a2"])
    }

    @Test
    fun theChoiceIsRememberedForThatAppOnThisDevice() {
        platform.preferences["beside-chat:a2"] = "c1"
        show(Route.App("a2", viewing = true))

        rule.waitUntil(5_000) { composerShown() }
    }
}
