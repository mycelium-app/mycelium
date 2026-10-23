package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.MyceliumApi
import mycelium.state.PickedFile
import mycelium.state.Store
import mycelium.state.projectLogoProblem
import mycelium.ui.ProjectIconSet
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

private const val Updated = """{"id":"p1","node_id":"n1","name":"app","path":"/p/app","icon":"rocket","created_at":"2026-10-22T10:00:00Z"}"""

class ProjectIconsTest {
    private val requests = mutableListOf<Triple<String, String, String>>()

    private fun store(scope: kotlinx.coroutines.CoroutineScope): Store {
        val engine = MockEngine { request ->
            requests += Triple(request.method.value, request.url.encodedPath, String(request.body.toByteArray()))

            respond(Updated, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }

        return Store(MyceliumApi("https://mycelium.test", "token", engine), scope)
    }

    @Test
    fun theIconSetHasUniqueNamesAMenuCanSend() {
        val names = ProjectIconSet.map { it.name }

        assertTrue(names.size >= 48)
        assertEquals(names.size, names.toSet().size)
        assertTrue(names.all { name -> name.all { it in 'a'..'z' || it == '-' } })
    }

    @Test
    fun aLogoMustBeANonEmptyImageUnder256Kib() {
        assertNull(projectLogoProblem(PickedFile("logo.png", "image/png", ByteArray(10) { 1 })))
        assertEquals("logo.svg is not a PNG, JPEG or WebP image. Choose another file.", projectLogoProblem(PickedFile("logo.svg", "image/svg+xml", ByteArray(10) { 1 })))
        assertEquals("notes.txt is not a PNG, JPEG or WebP image. Choose another file.", projectLogoProblem(PickedFile("notes.txt", "text/plain", ByteArray(10) { 1 })))
        assertEquals("logo.gif is not a PNG, JPEG or WebP image. Choose another file.", projectLogoProblem(PickedFile("logo.gif", "image/gif", ByteArray(10) { 1 })))
        assertEquals("empty.png is empty. Choose an image that has content.", projectLogoProblem(PickedFile("empty.png", "image/png", ByteArray(0))))
        assertEquals("big.png is larger than 256 KB. Choose a smaller image.", projectLogoProblem(PickedFile("big.png", "image/png", ByteArray(256 * 1024 + 1))))
    }

    @Test
    fun choosingAnIconPatchesItAndNoIconClearsIt() = runTest {
        val store = store(backgroundScope)

        val chosen = store.setProjectIcon("p1", "rocket")
        store.setProjectIcon("p1", null)

        assertEquals("rocket", chosen.icon)
        assertEquals(listOf("""{"icon":"rocket"}""", """{"clear_icon":true}"""), requests.map { it.third })
        assertEquals("/api/v1/projects/p1", requests.first().second)
    }

    @Test
    fun uploadingALogoPostsTheBytesWithTheirTypeAndKeepsTheAnswer() = runTest {
        val store = store(backgroundScope)

        store.uploadProjectAvatar("p1", "image/png", byteArrayOf(1, 2, 3))

        assertEquals(Triple("POST", "/api/v1/projects/p1/avatar", "\u0001\u0002\u0003"), requests.single())
        assertEquals("rocket", store.state.value.projects["p1"]?.icon)
    }
}
