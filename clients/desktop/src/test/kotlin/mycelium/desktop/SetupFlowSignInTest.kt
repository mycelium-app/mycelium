package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.SetupFlow
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Created = """{"id":"s1","name":"mcp.example.test","url":"https://mcp.example.test/mcp","enabled":true,"oauth":true,"tools":[],"created_at":"2026-10-27T10:00:00Z"}"""
private const val Registration = """{"oauth":true,"sign_in":"registration","authorization_server":"https://auth.example.test"}"""
private const val ClientId = """{"oauth":false,"sign_in":"client_id","authorization_server":"https://auth.example.test"}"""
private const val NotOffered = """{"oauth":false,"sign_in":"none","reason":"The server asks for sign-in but publishes no authorisation details."}"""
private const val AuthorizeUrl = "https://auth.example.test/authorize?state=si-1"

class SetupFlowSignInTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = CopyOnWriteArrayList<Triple<HttpMethod, String, String>>()
    private val platform = OpeningPlatform()
    private var probeAnswer = Registration
    private var statusAnswer = HttpStatusCode.OK to """{"state":"waiting"}"""
    private var finished = false

    private fun open() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = SynchronousMockEngine { request ->
            val path = request.url.encodedPath
            requests += Triple(request.method, path, String(request.body.toByteArray()))

            when {
                path.endsWith("/probe") -> respond(probeAnswer, HttpStatusCode.OK, json)
                path.endsWith("/chat-tool-servers/oauth") -> respond("""{"authorize_url":"$AuthorizeUrl","sign_in":"si-1"}""", HttpStatusCode.OK, json)
                path == "/integrations/oauth/si-1" && request.method == HttpMethod.Get -> respond(statusAnswer.second, statusAnswer.first, json)
                path == "/integrations/oauth/si-1" -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/test") -> respond("""{"tools":[{"name":"search","kind":"read"}]}""", HttpStatusCode.OK, json)
                else -> respond(Created, HttpStatusCode.Created, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.example.test", "token", engine), scopes.create())

        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalPlatform provides platform) { SetupFlow(store, "mcp", null, emptySet(), {}, { finished = true }, {}) }
            }
        }
        rule.onNodeWithContentDescription("Server address").performTextInput("https://mcp.example.test/mcp")
        rule.onNodeWithText("Continue").performClick()
        rule.waitForIdle()
    }

    private fun pollOnce() {
        rule.mainClock.advanceTimeBy(2_100)
        rule.waitForIdle()
    }

    private fun startSignIn() {
        rule.onNodeWithText("Sign in with auth.example.test").performClick()
        rule.waitForIdle()
    }

    @Test
    fun aRegistrationProbeOffersSignInFirstWithOneLineAndTheAuthorisationHost() {
        open()

        rule.onNodeWithText("mcp.example.test asks for a credential.").assertExists()
        rule.onNodeWithText("How to connect").assertExists()
        rule.onNodeWithText("Sign in").assertExists()
        rule.onNodeWithText("Token").assertExists()
        rule.onNodeWithText("Your browser opens auth.example.test. Mycelium connects the account when you finish there.").assertExists()
        rule.onNodeWithText("Sign in with auth.example.test").assertExists()
        rule.onNodeWithContentDescription("Expiration", substring = true).assertDoesNotExist()
    }

    @Test
    fun aProbeWithNoSignInShowsItsReasonAndOnlyTheTokenForm() {
        probeAnswer = NotOffered
        open()

        rule.onNodeWithText("The server asks for sign-in but publishes no authorisation details.").assertExists()
        rule.onNodeWithContentDescription("Token").assertExists()
        rule.onNodeWithText("How to connect").assertDoesNotExist()
        rule.onNodeWithText("Continue").assertExists()
    }

    @Test
    fun aClientIdProbeAsksForTheClientIdAndKeepsTheSecretHiddenUntilShown() {
        probeAnswer = ClientId
        open()

        rule.onNodeWithText("auth.example.test does not let apps register themselves. Register Mycelium there with the callback URL below, then enter the client ID it gives you.").assertExists()
        rule.onNodeWithContentDescription("Callback URL").assertExists()
        rule.onNodeWithText("https://mycelium.example.test/integrations/oauth/callback").assertExists()
        rule.onNodeWithText("Copy").assertExists()
        rule.onNodeWithContentDescription("Client ID").performTextInput("mycelium-sam")
        rule.onNodeWithContentDescription("Client secret (optional)").performTextInput("hunter2-secret")
        rule.onNode(hasContentDescription("Client secret (optional)") and SemanticsMatcher.keyIsDefined(SemanticsProperties.Password)).assertExists()
        rule.onNodeWithText("Hide").assertDoesNotExist()

        rule.onNodeWithText("Show").performClick()
        rule.onNodeWithText("Hide").assertExists()

        startSignIn()
        val started = requests.last { it.second.endsWith("/chat-tool-servers/oauth") }.third
        assertTrue(started.contains(""""client_id":"mycelium-sam"""") && started.contains(""""client_secret":"hunter2-secret""""), started)
    }

    @Test
    fun aMissingClientIdShowsTheErrorSummaryAndTheFieldErrorAndStartsNothing() {
        probeAnswer = ClientId
        open()

        startSignIn()

        rule.onNodeWithText("There is a problem").assertExists()
        rule.onAllNodesWithText("Enter the client ID from auth.example.test").assertCountEquals(2)
        assertTrue(requests.none { it.second.endsWith("/chat-tool-servers/oauth") })
    }

    @Test
    fun signingInStartsItOpensTheSignInPageAndWaitsForTheBrowser() {
        open()
        startSignIn()

        assertEquals(listOf(AuthorizeUrl), platform.opened)
        assertTrue(requests.any { it.second.endsWith("/chat-tool-servers/oauth") && it.third.contains(""""url":"https://mcp.example.test/mcp"""") })
        rule.onNodeWithText("Finish signing in at auth.example.test in your browser. This step moves on when you are done.").assertExists()
        rule.onNodeWithText("Cancel").assertExists()
        rule.onNodeWithText("Open the sign-in page again").assertExists()
        rule.onNodeWithText("Sign in with auth.example.test").assertDoesNotExist()
    }

    @Test
    fun openingTheSignInPageAgainReopensTheSameAddress() {
        open()
        startSignIn()

        rule.onNodeWithText("Open the sign-in page again").performClick()

        assertEquals(listOf(AuthorizeUrl, AuthorizeUrl), platform.opened)
    }

    @Test
    fun cancellingDeletesThePendingSignInAndReturnsToTheChoice() {
        open()
        startSignIn()

        rule.onNodeWithText("Cancel").performClick()
        rule.waitForIdle()

        assertTrue(requests.any { it.first == HttpMethod.Delete && it.second == "/integrations/oauth/si-1" })
        rule.onNodeWithText("Sign in with auth.example.test").assertExists()
        rule.onNodeWithText("Finish signing in at auth.example.test in your browser. This step moves on when you are done.").assertDoesNotExist()
    }

    @Test
    fun finishingInTheBrowserMovesToTestAndAddingSendsTheSignInInsteadOfAToken() {
        open()
        startSignIn()
        statusAnswer = HttpStatusCode.OK to """{"state":"connected","server":null}"""

        pollOnce()

        rule.onNodeWithContentDescription("Step 3 of 4: Test").assertExists()
        rule.onNodeWithText("Connected. Signed in at auth.example.test.").assertExists()

        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()
        rule.waitForIdle()

        assertTrue(finished)
        assertEquals("""{"name":"mcp.example.test","url":"https://mcp.example.test/mcp","sign_in":"si-1"}""", requests.last { it.second.endsWith("/chat-tool-servers") }.third)
    }

    @Test
    fun aRefusedSignInShowsTheReasonWithTheWayOutAndTryAgain() {
        open()
        startSignIn()
        statusAnswer = HttpStatusCode.OK to """{"state":"failed","reason":"auth.example.test refused the sign-in: access denied"}"""

        pollOnce()

        rule.onNodeWithText("auth.example.test refused the sign-in: access denied. Try again, or connect with a token.").assertExists()
        rule.onNodeWithText("Try again").assertExists()
        rule.onNodeWithText("Sign in").assertExists()
        rule.onNodeWithText("Token").assertExists()
    }

    @Test
    fun aSignInThatHasExpiredSaysSoAndOffersTryAgain() {
        open()
        startSignIn()
        statusAnswer = HttpStatusCode.NotFound to """{"error":"not_found","message":"That sign-in was not found."}"""

        pollOnce()

        rule.onNodeWithText("This sign-in has expired. Try again, or connect with a token.").assertExists()
        rule.onNodeWithText("Try again").assertExists()
    }

    @Test
    fun theTokenChoiceKeepsTheTokenFieldAndExpiryAndSendsTheToken() {
        open()

        rule.onNodeWithText("Token").performClick()
        rule.onNodeWithContentDescription("Token").performTextInput("secret-token")
        rule.onNodeWithContentDescription("Expiration", substring = true).assertExists()
        rule.onNodeWithText("Continue").performClick()
        rule.waitForIdle()
        rule.onNodeWithText("Continue").performClick()
        rule.onNodeWithText("Add integration").performClick()
        rule.waitForIdle()

        assertTrue(finished)
        assertEquals("""{"name":"mcp.example.test","url":"https://mcp.example.test/mcp","bearer_token":"secret-token"}""", requests.last { it.second.endsWith("/chat-tool-servers") }.third)
        assertTrue(requests.none { it.second.endsWith("/chat-tool-servers/oauth") })
    }
}
