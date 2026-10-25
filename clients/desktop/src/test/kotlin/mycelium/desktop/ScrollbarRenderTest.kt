package mycelium.desktop

import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventType
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.state.Store
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private const val Chat = "11111111-1111-1111-1111-111111111111"

private fun node(index: Int) =
    """{"id":"aaaaaaaa-0000-0000-0000-0000000000${"%02d".format(index)}","name":"node-$index","hostname":"node-$index","os":"linux","arch":"x86_64","version":"v0.1.13","update_available":"v0.3.6","online":true,"draining":false,
    "agents":[{"agent":"claude","version":"@agentclientprotocol/claude-agent-acp 0.94.0","available":true,"update_available":"0.95.1"},{"agent":"codex","version":"0.163.0","available":true}],
    "logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in"}]}"""

private fun message(seq: Int) =
    """{"conversation_id":"$Chat","seq":$seq,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Paragraph $seq of a long reply that keeps the thread scrolling so the thumb has somewhere to go."}}}}"""

@OptIn(InternalComposeUiApi::class)
class ScrollbarRenderTest {
    @get:Rule
    val scopes = TestScopes()
    private val me = Me("u", "sam@example.com", "Sam")

    private fun render(name: String, route: Route, extra: Map<String, String>, nodes: String, x: Float, y: Float) {
        val store = Store(fakeApi(nodes, extra), scopes.create())

        runBlocking {
            store.refresh()
            store.open(Chat)
        }

        val file = renderAt(name, true, false, interact = { scene: ImageComposeScene ->
            scene.render(0)
            scene.sendPointerEvent(PointerEventType.Move, Offset(x, y))
        }) { Shell(store, PlatformStub, me, onSignOut = {}, initialRoute = route) }

        assertTrue(file.length() > 10_000)
    }

    @Test
    fun theNodesPageAndTheChatRenderWithAScrollbarForReview() {
        render("scrollbar-nodes", Route.Nodes, emptyMap(), (1..14).joinToString(",", "[", "]") { node(it) }, 1000f, 400f)
        render("scrollbar-chat", Route.Chat(Chat), mapOf("/events" to (1..40).joinToString(",", "[", "]") { message(it) }), "[]", 1000f, 400f)
    }
}
