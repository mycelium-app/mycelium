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
    fun theChatRendersWithAScrollbarForReview() {
        render("scrollbar-chat", Route.Chat(Chat), mapOf("/events" to (1..40).joinToString(",", "[", "]") { message(it) }), "[]", 1000f, 400f)
    }
}
