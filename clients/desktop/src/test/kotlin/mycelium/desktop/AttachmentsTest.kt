package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.PickedFile
import mycelium.state.Store
import mycelium.state.formatSize
import mycelium.state.sha256Hex
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import java.awt.Color
import java.awt.image.BufferedImage
import java.io.ByteArrayOutputStream
import java.util.Base64
import javax.imageio.ImageIO
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val IdleJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Fix the build","status":"done",
"acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

internal fun testPng(width: Int = 96, height: Int = 64, tint: Int = 0x2f9e44): ByteArray {
    val image = BufferedImage(width, height, BufferedImage.TYPE_INT_RGB)

    for (x in 0 until width) {
        for (y in 0 until height) {
            image.setRGB(x, y, Color((tint shr 16) and 0xff, (x * 255 / width), (y * 255 / height)).rgb)
        }
    }

    return ByteArrayOutputStream().also { ImageIO.write(image, "png", it) }.toByteArray()
}

private class FilePlatform(private val picked: List<PickedFile> = emptyList(), private val clipboard: List<PickedFile> = emptyList()) : Platform by QuietPlatform {
    var listener: ((List<PickedFile>) -> Unit)? = null

    override fun pickFiles(onPicked: (List<PickedFile>) -> Unit) = onPicked(picked)

    override fun clipboardFiles(): List<PickedFile> = clipboard

    override fun listenForFiles(listener: ((List<PickedFile>) -> Unit)?) {
        this.listener = listener
    }
}

class AttachmentsTest {
    @get:Rule
    val rule = createComposeRule()

    private val png = testPng()

    private val requests = mutableListOf<Pair<HttpRequestData, String>>()

    private fun open(platform: Platform, events: String = "[]") {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += request to String(request.body.toByteArray())

            when {
                path.endsWith("/attachments") && request.method == HttpMethod.Post ->
                    respond("""{"sha256":"${sha256Hex(request.body.toByteArray())}","name":"${request.url.parameters["name"]}","media_type":"image/png","size":${request.body.toByteArray().size}}""", HttpStatusCode.Created, json)
                path.startsWith("/api/v1/attachments/") -> respond(png, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "image/png"))
                path.endsWith("/prompt") -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/conversations") -> respond("[$IdleJson]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(IdleJson, HttpStatusCode.OK, json)
                path.endsWith("/events") -> respond(events, HttpStatusCode.OK, json)
                path.endsWith("/queue") || path.endsWith("/nodes") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    @Test
    fun attachingAFileShowsAChipWithItsSizeAndRemovingItClearsTheChip() {
        open(FilePlatform(picked = listOf(PickedFile("report.pdf", "application/pdf", ByteArray(2048) { 1 }))))

        rule.onNodeWithContentDescription("Attach files").performClick()

        rule.onNodeWithContentDescription("Attached: report.pdf, 2 KB").assertIsDisplayed()
        rule.onNodeWithContentDescription("Remove attachment: report.pdf").performClick()
        assertEquals(0, rule.onAllNodesWithContentDescription("Attached: report.pdf, 2 KB").fetchSemanticsNodes().size)
    }

    @Test
    fun aFileOverTwentyMegabytesIsRefusedWithWhatToDoAndNoChipAppears() {
        open(FilePlatform(picked = listOf(PickedFile("big.bin", "application/octet-stream", ByteArray(20 * 1024 * 1024 + 1)))))

        rule.onNodeWithContentDescription("Attach files").performClick()

        rule.onNodeWithText("big.bin is larger than 20 MB. Choose a smaller file.").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Remove attachment: big.bin").fetchSemanticsNodes().size)
    }

    private fun pasteImage() {
        rule.onNodeWithContentDescription("Message").performClick()
        rule.onNodeWithContentDescription("Message").performKeyInput {
            keyDown(Key.CtrlLeft)
            pressKey(Key.V)
            keyUp(Key.CtrlLeft)
        }
    }

    @Test
    fun anImagePastedWithControlVShowsAsAnInlineBadgeNamedAfterTheFile() {
        open(FilePlatform(clipboard = listOf(PickedFile("pasted-image.png", "image/png", png))))

        pasteImage()

        rule.onNodeWithText("Image · pasted-image.png", substring = true).assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Attached: pasted-image.png, ${formatSize(png.size.toLong())}").fetchSemanticsNodes().size)
    }

    @Test
    fun backspaceAfterAnInlineImageBadgeRemovesTheAttachment() {
        open(FilePlatform(clipboard = listOf(PickedFile("pasted-image.png", "image/png", png))))

        pasteImage()
        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Backspace) }

        assertEquals(0, rule.onAllNodesWithText("Image · pasted-image.png", substring = true).fetchSemanticsNodes().size)

        rule.onNodeWithContentDescription("Send").performClick()

        assertEquals(0, requests.count { it.first.url.encodedPath.endsWith("/attachments") })
    }

    @Test
    fun filesDroppedOrPastedInTheBrowserArriveThroughTheListenerTheComposerRegisters() {
        val platform = FilePlatform()
        open(platform)

        rule.runOnIdle { platform.listener!!(listOf(PickedFile("notes.txt", "text/plain", "hello".encodeToByteArray()))) }

        rule.onNodeWithContentDescription("Attached: notes.txt, 5 B").assertIsDisplayed()
    }

    @Test
    fun sendingUploadsTheFileThenPostsThePromptThatNamesIt() {
        open(FilePlatform(picked = listOf(PickedFile("shot.png", "image/png", png))))

        rule.onNodeWithContentDescription("Attach files").performClick()
        rule.onNodeWithContentDescription("Message").performTextInput("what is this")
        rule.onNodeWithContentDescription("Send").performClick()
        rule.waitUntil(5_000) { requests.any { it.first.url.encodedPath.endsWith("/prompt") } }

        val posts = requests.filter { it.first.method == HttpMethod.Post }
        val upload = posts.first { it.first.url.encodedPath.endsWith("/attachments") }
        val prompt = posts.first { it.first.url.encodedPath.endsWith("/prompt") }

        assertEquals("shot.png", upload.first.url.parameters["name"])
        assertTrue(prompt.second.contains(""""text":"what is this""""))
        assertTrue(prompt.second.contains(sha256Hex(png)))
    }

    private fun thread(): String {
        val sha = sha256Hex(png)
        val data = Base64.getEncoder().encodeToString(png)
        val sent = """{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{"text":"what is wrong here","attachments":[{"sha256":"$sha","name":"shot.png","media_type":"image/png","size":${png.size}},{"sha256":"${"b".repeat(64)}","name":"report.pdf","media_type":"application/pdf","size":2048}]}}"""
        val read = """{"conversation_id":"c1","seq":2,"kind":"acp_update","payload":{"update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Read /work/shot.png","kind":"read","status":"completed","content":[{"type":"content","content":{"type":"image","data":"$data","mimeType":"image/png"}}]}}}"""

        return "[$sent,$read]"
    }

    @Test
    fun aSentImageShowsInTheMessageWithItsNameAsAltTextAndFilesShowAsChips() {
        open(FilePlatform(), thread())

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("shot.png").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("shot.png").assertIsDisplayed()
        rule.onNodeWithContentDescription("Attachment: report.pdf, 2 KB").assertIsDisplayed()
    }

    @Test
    fun theSameImageTheAgentReadsIsShownAsAReferenceNotAgain() {
        open(FilePlatform(), thread())

        rule.onNodeWithText("Same image as above").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Read /work/shot.png").fetchSemanticsNodes().size)
        rule.onNodeWithText("Same image as above").performClick()
    }

    @Test
    fun aThreadWithAPastedImageAndTheAgentReadingItAgainRendersToAPicture() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.startsWith("/api/v1/attachments/") -> respond(png, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "image/png"))
                path.endsWith("/events") -> respond(thread(), HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[$IdleJson]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(IdleJson, HttpStatusCode.OK, json)
                path.endsWith("/queue") || path.endsWith("/nodes") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), CoroutineScope(Dispatchers.Unconfined))
        runBlocking { store.refresh() }

        val render = renderAt("attachments", dark = true, phone = false, interact = { Thread.sleep(800) }) {
            Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1"))
        }

        assertTrue(render.length() > 10_000)
    }
}
