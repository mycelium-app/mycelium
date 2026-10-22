package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.DeviceKind
import mycelium.api.DeviceRequest
import mycelium.api.MyceliumApi
import mycelium.state.SignInRejected
import mycelium.state.normaliseAddress
import mycelium.state.signInWithDeviceCode
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull

private val Json = headersOf(HttpHeaders.ContentType, "application/json")

private const val DeviceBody = """{"device_code":"dc","user_code":"WDJB-MJHT","verification_uri":"https://x/approve",
"verification_uri_complete":"https://x/approve?code=WDJB-MJHT","interval":5,"expires_in":600}"""

private val Request = DeviceRequest(DeviceKind.Client, "Desktop", "host", "linux", "x86_64")

class DeviceSignInTest {
    @Test
    fun aPollThatCannotReachTheServerIsRetriedInsteadOfEndingTheSignIn() = runTest {
        var polls = 0
        val engine = MockEngine { request ->
            if (request.url.encodedPath.endsWith("/device")) {
                respond(DeviceBody, HttpStatusCode.OK, Json)
            } else {
                polls++

                if (polls == 1) {
                    throw kotlinx.io.IOException("Software caused connection abort")
                }

                respond("""{"kind":"client","id":"d1","token":"secret"}""", HttpStatusCode.OK, Json)
            }
        }

        val token = signInWithDeviceCode(MyceliumApi("https://x", engine = engine), Request) {}

        assertEquals("secret", token.token)
        assertEquals(2, polls)
    }

    @Test
    fun showsTheCodeThenReturnsTheTokenOnceApproved() = runTest {
        var polls = 0
        val engine = MockEngine { request ->
            if (request.url.encodedPath.endsWith("/device")) {
                respond(DeviceBody, HttpStatusCode.OK, Json)
            } else {
                polls++

                if (polls < 3) {
                    respond("""{"error":"authorization_pending","message":"Waiting."}""", HttpStatusCode.BadRequest, Json)
                } else {
                    respond("""{"kind":"client","id":"d1","token":"secret"}""", HttpStatusCode.OK, Json)
                }
            }
        }
        var shownCode: String? = null

        val token = signInWithDeviceCode(MyceliumApi("https://x", engine = engine), Request) { shownCode = it.userCode }

        assertEquals("WDJB-MJHT", shownCode)
        assertEquals("secret", token.token)
        assertEquals(3, polls)
    }

    @Test
    fun anExpiredCodeEndsWithASentenceSayingWhatToDo() = runTest {
        val engine = MockEngine { request ->
            if (request.url.encodedPath.endsWith("/device")) {
                respond(DeviceBody, HttpStatusCode.OK, Json)
            } else {
                respond("""{"error":"expired_token","message":"x"}""", HttpStatusCode.BadRequest, Json)
            }
        }

        val failure = assertFailsWith<SignInRejected> {
            signInWithDeviceCode(MyceliumApi("https://x", engine = engine), Request) {}
        }

        assertEquals("The code expired before it was approved. Select Continue to get a new one.", failure.message)
    }

    @Test
    fun addressWithoutASchemeGetsHttpsAndTrailingSlashesGo() {
        assertEquals("https://mycelium.example.com", normaliseAddress(" mycelium.example.com/ "))
        assertEquals("http://localhost:8080", normaliseAddress("http://localhost:8080"))
    }

    @Test
    fun blankOrSpacedAddressIsRejected() {
        assertNull(normaliseAddress("  "))
        assertNull(normaliseAddress("my host"))
    }
}
