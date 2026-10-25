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
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.Walkthrough
import mycelium.state.Store
import mycelium.state.changesTab
import mycelium.api.Me
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.ui.MyTheme
import mycelium.ui.WalkthroughView
import org.junit.Rule
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
    @get:Rule
    val scopes = TestScopes()
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
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
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

    private fun renderConversation(name: String, diffOpen: Boolean, click: androidx.compose.ui.geometry.Offset?) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val chat = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/backups","title":"Backups","status":"done","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/notes") -> RenderNotes
                path.endsWith("/uncovered") -> "[]"
                path.endsWith("/reads") -> "[]"
                path.endsWith("/walkthrough") -> RenderWalkthrough
                path.endsWith("/git/changes-layout") -> """{"files":[{"path":"src/restore/Screen.tsx","staged":false,"unstaged":true,"untracked":false,"added":3,"removed":2,"rows":6,"hunks":[0],"fingerprint":"f1"}]}"""
                path.endsWith("/git/changes-rows") -> """{"ranges":[{"path":"src/restore/Screen.tsx","from":0,"fingerprint":"f1","rows":[{"k":"h","t":"@@ -1,5 +1,5 @@"},{"k":"d","o":1,"t":"const title = 1"},{"k":"a","n":1,"t":"const title = 2"},{"k":"c","o":2,"n":2,"t":"const retry = useRetry(RESTORE_BACKOFF_MS)"},{"k":"d","o":3,"t":"const failed = 1"},{"k":"a","n":3,"t":"const failed = 2"}]}]}"""
                path.endsWith("/conversations") -> "[$chat]"
                path.endsWith("/conversations/c1") -> chat
                path.endsWith("/nodes") || path.endsWith("/events") || path.endsWith("/queue") -> "[]"
                path.endsWith("/goal") -> "null"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }

        if (diffOpen) {
            store.openDiffTab("c1", changesTab("src/restore/Screen.tsx"))
        }

        val file = renderAt(name, true, false, interact = { scene ->
            scene.render(100_000_000L)

            if (click != null) {
                scene.sendPointerEvent(androidx.compose.ui.input.pointer.PointerEventType.Move, click)
                scene.sendPointerEvent(androidx.compose.ui.input.pointer.PointerEventType.Press, click)
                scene.render(116_000_000L)
                scene.sendPointerEvent(androidx.compose.ui.input.pointer.PointerEventType.Release, click)
                scene.render(132_000_000L)
            }
        }) {
            Shell(store, HandsOffPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1"))
        }

        assertTrue(file.length() > 1_000)
    }

    @Test
    fun theWalkthroughTabSelectedInTheDesktopConversation() {
        renderConversation("conversation-walkthrough-tab", false, androidx.compose.ui.geometry.Offset(TabX, TabY))
    }

    @Test
    fun aDiffTabOpenBesideTheWalkthroughTab() {
        renderConversation("conversation-diff-tab", true, null)
    }
}

private const val TabX = 435f
private const val TabY = 113f
