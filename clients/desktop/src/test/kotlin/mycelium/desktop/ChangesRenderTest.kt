package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.Store
import mycelium.state.changesTab
import mycelium.ui.ChangesTabView
import mycelium.ui.MyTheme
import kotlin.test.Test
import kotlin.test.assertTrue

private const val Layout = """{"files":[
{"path":"Long.kt","staged":false,"unstaged":true,"untracked":false,"added":6,"removed":5,"rows":44,"hunks":[0,22],"fingerprint":"a"},
{"path":"Short.kt","staged":true,"unstaged":false,"untracked":false,"added":1,"removed":1,"rows":3,"hunks":[0],"fingerprint":"b"},
{"path":"Fresh.kt","staged":false,"unstaged":false,"untracked":true,"added":4,"removed":0,"rows":5,"hunks":[0],"fingerprint":"c"}]}"""

private fun context(index: Int) = """{"k":"c","o":$index,"n":$index,"t":"    val step$index = compute(step${index - 1}) // keep"}"""

private val LongRows = (listOf("""{"k":"h","t":"@@ -1,22 +1,22 @@ fun restore()"}""") +
    (1..9).map { context(it) } +
    listOf("""{"k":"d","o":10,"t":"    val title = \"Restore your backup\""}""", """{"k":"a","n":10,"t":"    val title = strings.restore.title"}""") +
    (11..20).map { context(it) } +
    listOf("""{"k":"h","t":"@@ -40,20 +40,20 @@ fun retry()"}""") +
    (41..50).map { context(it) } +
    listOf("""{"k":"d","o":51,"t":"    return failed(\"Could not restore\")"}""", """{"k":"a","n":51,"t":"    return failed(strings.restore.failed)"}""") +
    (52..60).map { context(it) }).take(44)

private val Rows = """{"ranges":[
{"path":"Long.kt","from":0,"fingerprint":"a","rows":[${LongRows.joinToString(",")}]},
{"path":"Short.kt","from":0,"fingerprint":"b","rows":[{"k":"h","t":"@@ -1 +1 @@"},{"k":"d","o":1,"t":"val old = 1"},{"k":"a","n":1,"t":"val new = 2"}]},
{"path":"Fresh.kt","from":0,"fingerprint":"c","rows":[{"k":"h","t":"@@ -0,0 +1,4 @@"},{"k":"a","n":1,"t":"package fresh"},{"k":"a","n":2,"t":""},{"k":"a","n":3,"t":"fun hello() = \"hello\""},{"k":"a","n":4,"t":"val answer = 42"}]}]}"""

@OptIn(androidx.compose.ui.InternalComposeUiApi::class)
class ChangesRenderTest {
    @Test
    fun threeFilesOneLongOneCollapsedOneUntracked() {
        val conversation = MyceliumJson.decodeFromString<Conversation>("""{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work","title":"Importer","status":"review","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}""")
        val engine = MockEngine { request ->
            val body = when {
                request.url.encodedPath.endsWith("/git/changes-layout") -> Layout
                request.url.encodedPath.endsWith("/git/changes-rows") -> Rows
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))

        val file = renderAt("changes-view", true, false, interact = { scene ->
            for (frame in 1..40) {
                scene.render(frame * 16_000_000L)
                Thread.sleep(25)
            }

            for (key in listOf(Key.N, Key.N, Key.Enter)) {
                scene.sendKeyEvent(KeyEvent(key, KeyEventType.KeyDown))
                scene.sendKeyEvent(KeyEvent(key, KeyEventType.KeyUp))
                scene.render(1_000_000_000L)
            }
        }) {
            Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) { ChangesTabView(store, conversation, changesTab()) }
        }

        assertTrue(file.length() > 1_000)
    }
}
