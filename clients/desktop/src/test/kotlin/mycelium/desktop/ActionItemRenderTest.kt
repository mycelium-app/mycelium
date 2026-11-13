package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpStatusCode
import kotlin.io.encoding.Base64
import kotlin.time.Clock
import kotlin.time.Duration.Companion.seconds
import java.io.File
import mycelium.api.ActionItem
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.TerminalFeed
import mycelium.ui.ActionItemCard
import mycelium.ui.InboxRequest
import mycelium.ui.MyceliumTheme
import org.jetbrains.skia.EncodedImageFormat
import kotlin.test.Test

@OptIn(InternalComposeUiApi::class)
class ActionItemRenderTest {
    @get:org.junit.Rule
    val scopes = TestScopes()

    private val store by lazy { Store(MyceliumApi("https://mycelium.test", "token", MockEngine { respond("{}", HttpStatusCode.OK) }), scopes.create()) }

    private fun item(run: String? = null) = MyceliumJson.decodeFromString(ActionItem.serializer(), commandItemJson(run))

    private fun render(name: String, request: InboxRequest.ActionItem, state: AppState = AppState()) {
        val directory = System.getenv("ACTION_ITEM_SHOTS") ?: return
        val scene = ImageComposeScene(640, 420, Density(1f)) {
            MyceliumTheme(true) {
                Box(Modifier.width(640.dp).padding(16.dp)) { ActionItemCard(store, state, "c1", request) {} }
            }
        }

        var image = scene.render(0)

        for (frame in 1..6) {
            image = scene.render(frame * 16_000_000L)
        }

        val out = File(directory, "$name.png")
        out.parentFile.mkdirs()
        out.writeBytes(image.encodeToData(EncodedImageFormat.PNG)!!.bytes)
        scene.close()
    }

    private val pushed = "To git.example.com:acme/acme-backups.git"

    @Test
    fun theFiveStatesRender() {
        val started = (Clock.System.now() - 4.seconds).toString()
        val running = item(runJson("running", startedAt = started))
        val feed = TerminalFeed().with(Base64.encode("$pushed\r\n".encodeToByteArray()))

        render("5.41-waiting", InboxRequest.ActionItem(item()))
        render("5.42-running", InboxRequest.ActionItem(running), AppState(terminalOutput = mapOf("t1" to feed)))
        render("5.43-succeeded", InboxRequest.ActionItem(item(runJson("running", output = "$pushed\n - [deleted]         feature/old-sync")), succeeded = true))
        render("5.44-failed", InboxRequest.ActionItem(item(runJson("failed", exitCode = 1, output = "error: unable to delete 'feature/old-sync': remote ref does not exist\nerror: failed to push some refs to 'git.example.com:acme/acme-backups.git'"))))
        render("5.45-done", InboxRequest.ActionItem(item(), done = true))
    }
}
