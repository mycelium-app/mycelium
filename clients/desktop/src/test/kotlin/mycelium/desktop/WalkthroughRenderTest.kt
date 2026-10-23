package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.Walkthrough
import mycelium.state.Store
import mycelium.ui.MyTheme
import mycelium.ui.WalkthroughView
import org.junit.Test
import kotlin.test.assertTrue

private const val RenderDiff = "@@ -1,5 +1,5 @@\\n-  const title = \\\"Restore your backup\\\"\\n+  const title = strings.restore.title\\n   const retry = useRetry(RESTORE_BACKOFF_MS)\\n-  const failed = \\\"Could not restore. Try again.\\\"\\n+  const failed = strings.restore.failed"

private const val RenderWalkthrough = """{"id":"w1","conversation_id":"c1","created_at":"2026-10-23T10:00:00Z","steps":[
{"title":"Restore reads its copy from strings","key":true,"section":"Strings","icon":"data","explanation":"The restore screen no longer holds its own copy, so a change to the wording reaches every screen at once.","files":[{"path":"src/restore/Screen.tsx","diff":"$RenderDiff"},{"path":"src/restore/untouched.ts","diff":"","unchanged":true}]},
{"title":"Update the retry screen","section":"Retry logic","icon":"logic"},
{"title":"Cover the restore flow","section":"Tests","icon":"tests"}]}"""

private const val RenderNotes = """[
{"id":"n1","step":0,"file":"src/restore/Screen.tsx","line_start":2,"line_end":2,"quote":"const title = strings.restore.title","comment":"Keep the old wording until design signs off","sent":false},
{"id":"n2","step":0,"comment":"Is untouched.ts meant to be in this walkthrough?","sent":false}]"""

private const val RenderUncovered = """[{"path":"src/restore/Backoff.ts","diff":"@@ -3,1 +3,1 @@\n-export const BACKOFF = 1000\n+export const BACKOFF = 5000","unchanged":false}]"""

@OptIn(ExperimentalComposeUiApi::class)
class WalkthroughRenderTest {
    private fun render(name: String, interact: (androidx.compose.ui.ImageComposeScene) -> Unit = {}) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/notes") -> RenderNotes
                path.endsWith("/uncovered") -> RenderUncovered
                path.endsWith("/reads") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        val walkthrough = MyceliumJson.decodeFromString(Walkthrough.serializer(), RenderWalkthrough)

        val file = renderAt(name, true, false, interact = interact) {
            Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) { WalkthroughView(store, "c1", walkthrough) }
        }

        assertTrue(file.length() > 1_000)
    }

    @Test
    fun walkthroughWithDiffAnnotationNoteAndNotCoveredSection() {
        render("walkthrough-notes")
    }

    @Test
    fun theNotCoveredYetStepListsTheLeftOverHunks() {
        render("walkthrough-not-covered") { scene ->
            scene.render(100_000_000L)
            scene.sendPointerEvent(androidx.compose.ui.input.pointer.PointerEventType.Move, androidx.compose.ui.geometry.Offset(140f, 278f))
            scene.sendPointerEvent(androidx.compose.ui.input.pointer.PointerEventType.Press, androidx.compose.ui.geometry.Offset(140f, 278f))
            scene.render(116_000_000L)
            scene.sendPointerEvent(androidx.compose.ui.input.pointer.PointerEventType.Release, androidx.compose.ui.geometry.Offset(140f, 278f))
            scene.render(132_000_000L)
        }
    }
}
