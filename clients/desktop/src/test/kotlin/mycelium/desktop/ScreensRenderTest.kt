package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.unit.Density
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.Accent
import mycelium.ui.Appearance
import mycelium.ui.Bar
import mycelium.ui.LocalAppearance
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Segmented
import mycelium.ui.Shell
import mycelium.ui.SignInScreen
import org.jetbrains.skia.EncodedImageFormat
import java.io.File
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertTrue

private const val ConversationId = "11111111-1111-1111-1111-111111111111"
private const val LaptopId = "aaaaaaaa-0000-0000-0000-000000000001"
private const val AtlasId = "aaaaaaaa-0000-0000-0000-000000000002"

private fun conversation(id: String, title: String, status: String, agent: String = "claude", node: String = LaptopId, archived: Boolean = false) =
    """{"id":"$id","owner":"u","node_id":"$node","agent":"$agent","cwd":"/home/sam/projects/midden","title":"$title","status":"$status",
    "acp_session_id":null,"mode":"acceptEdits","model":"Opus 5.5","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T11:${if (id == ConversationId) "59" else "10"}:00Z",
    "archived_at":${if (archived) "\"2026-10-22T11:00:00Z\"" else "null"}}"""

private fun update(seq: Int, body: String) =
    """{"conversation_id":"$ConversationId","seq":$seq,"kind":"acp_update","payload":{"sessionId":"s","update":$body}}"""

private val Events = listOf(
    """{"conversation_id":"$ConversationId","seq":1,"kind":"user_message","payload":{"text":"Bump @immich/ui to the latest release and make sure nothing else in the tree drifts from it."}}""",
    update(2, """{"sessionUpdate":"tool_call","toolCallId":"t1","title":"recall","kind":"other","status":"completed","rawInput":{"command":"\"@immich/ui version\", groups: [\"work\"]"},"content":[{"type":"content","content":{"type":"text","text":"1 memory"}}]}"""),
    update(3, """{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A **work** memory says to keep `@immich/ui` in step with the `@acme-org/backups-*` packages. They pin 0.31.2, so I'll match that rather than take the newest release."}}"""),
    update(4, """{"sessionUpdate":"tool_call","toolCallId":"t2","title":"Edit package.json","kind":"edit","status":"completed","content":[{"type":"diff","path":"package.json","oldText":"  \"dependencies\": {\n    \"@immich/ui\": \"^0.29.0\",\n    \"kysely\": \"^0.28.2\",","newText":"  \"dependencies\": {\n    \"@immich/ui\": \"0.31.2\",\n    \"kysely\": \"^0.28.2\","}]}"""),
    update(5, """{"sessionUpdate":"tool_call","toolCallId":"t3","title":"pnpm install","kind":"execute","status":"completed","rawInput":{"command":"pnpm install"},"content":[{"type":"content","content":{"type":"text","text":"12s"}}]}"""),
    update(6, """{"sessionUpdate":"tool_call","toolCallId":"t4","title":"pnpm -r test","kind":"execute","status":"in_progress","rawInput":{"command":"pnpm -r test"}}"""),
    """{"conversation_id":"$ConversationId","seq":7,"kind":"permission_request","payload":{"request_id":"r1","toolCall":{"toolCallId":"t5","title":"git commit -m \"fix(deps): pin @immich/ui to 0.31.2\""},"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"deny","name":"Reject","kind":"reject_once"}]}}""",
    """{"conversation_id":"$ConversationId","seq":8,"kind":"workflow","payload":{"run_id":"wf1","line":{"type":"started","key":"a","label":"New user","phase":"Build"}}}""",
    """{"conversation_id":"$ConversationId","seq":9,"kind":"workflow","payload":{"run_id":"wf1","line":{"type":"started","key":"b","label":"Machines and logins","phase":"Build"}}}""",
    """{"conversation_id":"$ConversationId","seq":10,"kind":"workflow","payload":{"run_id":"wf1","line":{"type":"result","key":"a","result":"ok"}}}""",
).joinToString(",", "[", "]")

private val Nodes = """[
{"id":"$LaptopId","name":"laptop","hostname":"laptop","os":"macos","arch":"aarch64","version":"0.1.0","online":true,"draining":false,
 "agents":[{"agent":"claude","version":"2.1.308","available":true},{"agent":"codex","version":"0.163.0","available":true}],
 "logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in"}]},
{"id":"$AtlasId","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,
 "agents":[{"agent":"codex","version":"0.163.0","available":true}],
 "logins":[{"agent":"codex","account":"sam@example.com","method":"device-auth","state":"expired"}]}]"""

private val Conversations = listOf(
    conversation("22222222-2222-2222-2222-222222222222", "Release v0.3.7 for outline", "input", node = AtlasId),
    conversation("33333333-3333-3333-3333-333333333333", "Renovate sweep", "failed", node = AtlasId),
    conversation("44444444-4444-4444-4444-444444444444", "Restore flow copy", "review"),
    conversation(ConversationId, "Bump @immich/ui in midden", "working"),
    conversation("55555555-5555-5555-5555-555555555555", "Uploads survive a 503 and a timeout", "working", agent = "codex", node = AtlasId),
    conversation("66666666-6666-6666-6666-666666666666", "Flux image automation for vm", "done", node = AtlasId),
).joinToString(",", "[", "]")

@androidx.compose.runtime.Composable
private fun withAccents(dark: Boolean, content: @androidx.compose.runtime.Composable () -> Unit) {
    CompositionLocalProvider(LocalAppearance provides Appearance(dark, Accent.Neutral, Accent.entries.filter { it != Accent.System }, {}), content = content)
}

internal val PlatformStub = object : Platform {
    override val signsInThroughTheBrowser = false
    override val originUrl: String? = null
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

internal fun fakeApi(nodes: String = Nodes, extra: Map<String, String> = emptyMap()): MyceliumApi {
    val json = headersOf(HttpHeaders.ContentType, "application/json")
    val engine = MockEngine { request ->
        val path = request.url.encodedPath
        val body = when {
            extra.keys.any { path.endsWith(it) } -> extra.entries.first { path.endsWith(it.key) }.value
            path.endsWith("/events") -> Events
            path.endsWith("/queue") -> "[]"
            path.endsWith("/nodes") -> nodes
            path.endsWith("/conversations") && request.url.parameters["archived"] == "true" -> "[]"
            path.endsWith("/conversations") -> Conversations
            else -> "{}"
        }

        respond(body, HttpStatusCode.OK, json)
    }

    return MyceliumApi("https://mycelium.test", "token", engine)
}

@OptIn(InternalComposeUiApi::class)
internal fun renderAt(name: String, dark: Boolean, phone: Boolean, tabs: Int = 0, interact: (ImageComposeScene) -> Unit = {}, content: @androidx.compose.runtime.Composable () -> Unit): File {
    val scene = ImageComposeScene(if (phone) 390 else 1440, if (phone) 844 else 900, Density(1f)) {
        MyceliumTheme(dark) { CompositionLocalProvider(LocalTouch provides phone, content = content) }
    }
    var image = scene.render(0)
    interact(scene)

    repeat(tabs) {
        scene.sendKeyEvent(KeyEvent(Key.Tab, KeyEventType.KeyDown))
        scene.sendKeyEvent(KeyEvent(Key.Tab, KeyEventType.KeyUp))
    }

    for (frame in 1..6) {
        image = scene.render(frame * 16_000_000L)
    }

    val bytes = image.encodeToData(EncodedImageFormat.PNG)!!.bytes
    val out = File("build/screens/${if (phone) "phone-" else ""}$name-${if (dark) "dark" else "light"}.png")
    out.parentFile.mkdirs()
    out.writeBytes(bytes)
    scene.close()

    return out
}

private fun render(name: String, dark: Boolean, content: @androidx.compose.runtime.Composable () -> Unit): File = renderAt(name, dark, false, content = content)

class ScreensRenderTest {
    @get:Rule
    val scopes = TestScopes()

    private val me = Me("u", "sam@example.com", "Sam")

    private fun shell(route: Route, nodes: String = Nodes): @androidx.compose.runtime.Composable () -> Unit {
        val store = Store(fakeApi(nodes), scopes.create())

        runBlocking {
            store.refresh()
            store.open(ConversationId)
        }

        return { Shell(store, PlatformStub, me, onSignOut = {}, initialRoute = route) }
    }

    @Test
    fun conversationRendersInBothThemes() {
        for (dark in listOf(true, false)) {
            assertTrue(render("conversation", dark, shell(Route.Chat(ConversationId))).length() > 10_000)
        }
    }

    @Test
    fun nodesNewAndArchiveRenderInBothThemes() {
        for (dark in listOf(true, false)) {
            assertTrue(render("nodes", dark, shell(Route.Nodes)).length() > 10_000)
            assertTrue(render("new", dark, shell(Route.New)).length() > 10_000)
            assertTrue(render("archive", dark, shell(Route.Archive)).length() > 10_000)
            assertTrue(render("settings", dark) { withAccents(dark) { shell(Route.Settings)() } }.length() > 10_000)
        }
    }

    @Test
    fun phoneScreensRenderInBothThemes() {
        for (dark in listOf(true, false)) {
            assertTrue(renderAt("conversation", dark, true, content = shell(Route.Chat(ConversationId))).length() > 10_000)
            assertTrue(renderAt("nodes", dark, true, content = shell(Route.Nodes)).length() > 10_000)
            assertTrue(renderAt("archive", dark, true, content = shell(Route.Archive)).length() > 10_000)
            assertTrue(renderAt("empty", dark, true, content = shell(Route.Empty)).length() > 5_000)
            assertTrue(renderAt("settings", dark, true) { withAccents(dark) { shell(Route.Settings)() } }.length() > 5_000)
            assertTrue(renderAt("signin", dark, true) { SignInScreen(PlatformStub, "mycelium.example.com", null) { _, _ -> } }.length() > 5_000)
        }
    }

    @Test
    fun newConversationWithoutNodesAndFocusedSegmentRender() {
        for (dark in listOf(true, false)) {
            for (phone in listOf(false, true)) {
                assertTrue(renderAt("new-no-node", dark, phone, content = shell(Route.New, "[]")).length() > 5_000)
                assertTrue(renderAt("segmented-focus", dark, phone, tabs = 2) { Bar { Segmented(listOf("All", "Done", "Archived"), "Done", {}, groupLabel = "State") } }.length() > 1_000)
            }
        }
    }

    @Test
    fun signInRendersInBothThemes() {
        for (dark in listOf(true, false)) {
            assertTrue(render("signin", dark) { SignInScreen(PlatformStub, "mycelium.example.com", null) { _, _ -> } }.length() > 5_000)
        }
    }
}
