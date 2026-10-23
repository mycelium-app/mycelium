package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.test.runTest
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.PickedFile
import mycelium.state.Store
import mycelium.state.ThreadItem
import mycelium.state.attachmentProblem
import mycelium.state.buildThread
import mycelium.state.formatSize
import mycelium.state.mediaTypeOf
import mycelium.state.sha256Hex
import kotlin.io.encoding.Base64
import kotlin.io.encoding.ExperimentalEncodingApi
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull

private val Png = byteArrayOf(0x89.toByte(), 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3)

private val OtherPng = byteArrayOf(0x89.toByte(), 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 9, 9, 9)

@OptIn(ExperimentalEncodingApi::class)
private fun base64(bytes: ByteArray) = Base64.encode(bytes)

private fun event(seq: Long, kind: String, payload: String) = Event("c1", seq, kind, MyceliumJson.parseToJsonElement(payload))

private fun userWith(seq: Long, bytes: ByteArray, name: String): Event {
    val attachment = """{"sha256":"${sha256Hex(bytes)}","name":"$name","media_type":"image/png","size":${bytes.size}}"""

    return event(seq, EventKinds.UserMessage, """{"text":"look at this","attachments":[$attachment]}""")
}

private fun readImage(seq: Long, callId: String, bytes: ByteArray, title: String = "Read /work/shot.png"): Event {
    val content = """[{"type":"content","content":{"type":"image","data":"${base64(bytes)}","mimeType":"image/png"}}]"""

    return event(seq, EventKinds.AcpUpdate, """{"update":{"sessionUpdate":"tool_call","toolCallId":"$callId","title":"$title","kind":"read","status":"completed","content":$content}}""")
}

class AttachmentsTest {
    @Test
    fun sha256MatchesTheKnownDigestsOfTheEmptyAndTheAbcInputs() {
        assertEquals("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", sha256Hex(ByteArray(0)))
        assertEquals("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", sha256Hex("abc".encodeToByteArray()))
        assertEquals("248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1", sha256Hex("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq".encodeToByteArray()))
    }

    @Test
    fun anImageIsRecognisedByItsBytesBeforeItsNameOrDeclaredType() {
        assertEquals("image/png", mediaTypeOf("photo.txt", Png, "text/plain"))
        assertEquals("text/markdown", mediaTypeOf("notes.md", "# hi".encodeToByteArray()))
        assertEquals("application/octet-stream", mediaTypeOf("data", byteArrayOf(0, 1)))
    }

    @Test
    fun sizesAreShownInTheUnitAPersonReads() {
        assertEquals("12 B", formatSize(12))
        assertEquals("340 KB", formatSize(340L * 1024))
        assertEquals("1.5 MB", formatSize(1_572_864))
    }

    @Test
    fun aFileOverTheLimitIsRefusedWithWhatToDo() {
        val big = PickedFile("big.bin", "application/octet-stream", ByteArray(20 * 1024 * 1024 + 1))

        assertEquals("big.bin is larger than 20 MB. Choose a smaller file.", attachmentProblem(big, 0))
        assertEquals("empty.txt is empty. Choose a file that has content.", attachmentProblem(PickedFile("empty.txt", "text/plain", ByteArray(0)), 0))
        assertNull(attachmentProblem(PickedFile("ok.txt", "text/plain", byteArrayOf(1)), 0))
        assertEquals("Attach no more than 10 files to one message. Remove one to add ok.txt.", attachmentProblem(PickedFile("ok.txt", "text/plain", byteArrayOf(1)), 10))
    }

    @Test
    fun aSentImageAndTheSameImageTheAgentReadsAreRecognisedAsOne() {
        val thread = buildThread(listOf(userWith(1, Png, "shot.png"), readImage(2, "t1", Png)))
        val sent = assertIs<ThreadItem.UserMessage>(thread[0])
        val read = assertIs<ThreadItem.ToolCall>(thread[1])

        assertNull(sent.images.single().sameAs)
        assertEquals("e1", read.images.single().sameAs)
    }

    @Test
    fun aDifferentImageIsShownInFullAndTheSameOneTwiceInTheAgentsHandsRefersToTheFirst() {
        val thread = buildThread(listOf(readImage(1, "t1", Png), readImage(2, "t2", OtherPng), readImage(3, "t3", Png)))
        val images = thread.filterIsInstance<ThreadItem.ToolCall>().map { it.images.single().sameAs }

        assertEquals(listOf(null, null, "e1"), images)
    }

    @Test
    fun aMessageKeepsItsNonImageFilesApartFromItsPictures() {
        val file = """{"sha256":"${"a".repeat(64)}","name":"report.pdf","media_type":"application/pdf","size":2048}"""
        val thread = buildThread(listOf(event(1, EventKinds.UserMessage, """{"text":"","attachments":[$file]}""")))
        val message = assertIs<ThreadItem.UserMessage>(thread.single())

        assertEquals(listOf("report.pdf"), message.files.map { it.name })
        assertEquals(emptyList(), message.images)
    }

    @Test
    fun anImageInAToolCardCarriesTheToolTitleAsItsName() {
        val read = assertIs<ThreadItem.ToolCall>(buildThread(listOf(readImage(1, "t1", Png))).single())

        assertEquals("Read /work/shot.png", read.images.single().name)
    }

    @Test
    fun sendingFilesUploadsEachThenPostsThePromptNamingThem() = runTest {
        val seen = mutableListOf<Triple<HttpMethod, String, String>>()
        val engine = MockEngine { request ->
            seen += Triple(request.method, request.url.encodedPath + "?" + request.url.encodedQuery, String(request.body.toByteArray()))

            if (request.url.encodedPath.endsWith("/attachments")) {
                respond("""{"sha256":"abc","name":"shot.png","media_type":"image/png","size":11}""", HttpStatusCode.Created, headersOf(HttpHeaders.ContentType, "application/json"))
            } else {
                respond("", HttpStatusCode.NoContent)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", null, engine), backgroundScope)

        store.send("c1", "what is this", listOf(PickedFile("shot.png", "image/png", Png)))
        withContext(Dispatchers.Default) { withTimeout(5_000) { while (seen.size < 2) delay(10) } }

        assertEquals("/api/v1/conversations/c1/attachments?name=shot.png", seen[0].second)
        assertEquals("/api/v1/conversations/c1/prompt?", seen[1].second)
        assertEquals("""{"text":"what is this","attachments":[{"sha256":"abc","name":"shot.png","media_type":"image/png","size":11}]}""", seen[1].third)
    }
}
