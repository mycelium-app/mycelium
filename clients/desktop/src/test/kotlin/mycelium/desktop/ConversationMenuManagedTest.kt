package mycelium.desktop

import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.ComposeUiTest
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.runDesktopComposeUiTest
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Groups = """[{"id":"g1","name":"slop","folders":[]}]"""

private fun assistant(id: String, name: String, group: String?) =
    """{"id":"$id","name":"$name","group_id":${group?.let { "\"$it\"" }},"conversation_id":"m-$id","created_at":"2026-10-01T00:00:00Z"}"""

private fun menuConversation(id: String, title: String, group: String? = "g1", managedBy: String? = null, assistantId: String? = null) =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"$title","status":"working","group_id":${group?.let { "\"$it\"" }},
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z","archived_at":null${managedBy?.let { ",\"managed_by\":\"$it\"" } ?: ""}${assistantId?.let { ",\"assistant_id\":\"$it\"" } ?: ""}}"""

@OptIn(ExperimentalTestApi::class)
class ConversationMenuManagedTest {
    @get:Rule
    val scopes = TestScopes()

    private val patches = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private fun ComposeUiTest.showChat(assistants: List<String>, chat: String = menuConversation("c1", "Bump the UI kit to 0.31.2"), extra: List<String> = emptyList()) {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Patch) {
                patches += path to String(request.body.toByteArray())

                return@MockEngine respond(menuConversation("c1", "Bump the UI kit to 0.31.2", managedBy = "m-a1"), HttpStatusCode.OK, JsonHeaders)
            }

            val body = when {
                path.endsWith("/conversations") -> "[${(listOf(chat) + extra).joinToString(",")}]"
                path.endsWith("/assistants") -> "[${assistants.joinToString(",")}]"
                path.endsWith("/groups") -> Groups
                path.endsWith("/nodes") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, JsonHeaders)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        setContent { MyceliumTheme(true) { Shell(store, AppsPlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
        waitUntil(timeoutMillis = 5_000) { onAllNodesWithText("Bump the UI kit to 0.31.2").fetchSemanticsNodes().isNotEmpty() }
        onNodeWithContentDescription("More").performClick()
    }

    @Test
    fun theHeaderMenuLetsTheOnlyAssistantOfTheGroupManageTheConversation() = runDesktopComposeUiTest(width = 1440, height = 900) {
        showChat(listOf(assistant("a1", "Infrastructure", "g1"), assistant("a2", "Elsewhere", "g2")))
        saveShot("2.61-conversation-header-menu-let-an-assistant-manage-this")

        onNodeWithText("Let Infrastructure manage this").performClick()

        waitUntil(timeoutMillis = 5_000) { patches.isNotEmpty() }
        assertEquals("/api/v1/conversations/c1" to """{"manage_by_assistant":"a1"}""", patches.single())
    }

    @Test
    fun withSeveralAssistantsInTheGroupTheMenuItemOpensASubmenuOfThem() = runDesktopComposeUiTest(width = 1440, height = 900) {
        showChat(listOf(assistant("a1", "Infrastructure", "g1"), assistant("a2", "Release captain", "g1")))

        onNodeWithText("Let an assistant manage this").performClick()
        onAllNodesWithText("Release captain").onLast().performClick()

        waitUntil(timeoutMillis = 5_000) { patches.isNotEmpty() }
        assertEquals("""{"manage_by_assistant":"a2"}""", patches.single().second)
    }

    @Test
    fun theMenuOffersNoAssistantWhenTheGroupHasNone() = runDesktopComposeUiTest(width = 1440, height = 900) {
        showChat(listOf(assistant("a2", "Elsewhere", "g2")))

        onNodeWithText("Snooze").assertExists()
        onAllNodesWithText("manage this", substring = true).assertCountEquals(0)
    }

    @Test
    fun aManagedConversationOffersTakeOverFromItsAssistantAndSaysNothingAboutManaging() = runDesktopComposeUiTest(width = 1440, height = 900) {
        val manager = menuConversation("m-a1", "Infrastructure chat", assistantId = "a1")
        showChat(listOf(assistant("a1", "Infrastructure", "g1")), menuConversation("c1", "Bump the UI kit to 0.31.2", managedBy = "m-a1"), listOf(manager))
        saveShot("1.36-conversation-managed-by-infrastructure")

        onNodeWithText("from Infrastructure").assertExists()
        onNodeWithText("Take over").performClick()

        waitUntil(timeoutMillis = 5_000) { patches.isNotEmpty() }
        assertEquals("""{"take_over":true}""", patches.single().second)
        onAllNodesWithText("manage this", substring = true).assertCountEquals(0)
    }
}
