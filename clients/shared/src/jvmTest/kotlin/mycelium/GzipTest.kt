package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.MyceliumApi
import java.io.ByteArrayOutputStream
import java.util.zip.GZIPOutputStream
import kotlin.test.Test
import kotlin.test.assertEquals

class GzipTest {
    private val conversation = """{"id":"c1","owner":"u","node_id":"n","agent":"claude","cwd":"/work","title":"Restore","status":"done","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

    private fun gzipped(text: String): ByteArray {
        val bytes = ByteArrayOutputStream()
        GZIPOutputStream(bytes).use { it.write(text.encodeToByteArray()) }

        return bytes.toByteArray()
    }

    @Test
    fun theAppAsksForGzipAndReadsAGzippedConversationList() = runTest {
        var accepted: String? = null
        val engine = MockEngine { request ->
            accepted = request.headers[HttpHeaders.AcceptEncoding]
            respond(gzipped("[$conversation]"), HttpStatusCode.OK, headersOf(HttpHeaders.ContentType to listOf("application/json"), HttpHeaders.ContentEncoding to listOf("gzip")))
        }

        val conversations = MyceliumApi("https://mycelium.test", null, engine).conversations()

        assertEquals(true, accepted?.contains("gzip"), accepted)
        assertEquals(listOf("c1"), conversations.map { it.id })
    }
}
