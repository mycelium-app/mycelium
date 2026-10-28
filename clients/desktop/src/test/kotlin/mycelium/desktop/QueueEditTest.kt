package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
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
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.sha256Hex
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val WorkingJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Fix the build","status":"working",
"acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private const val RunningTurn = """[{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{"text":"go"}}]"""

class QueueEditTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val png = testPng()
    private val sha = sha256Hex(png)

    private val requests = CopyOnWriteArrayList<Pair<HttpRequestData, String>>()

    private val queued = """[
        {"id":"q1","conversation_id":"c1","position":1,"text":"Once the tests pass, bump the docs site","created_at":"2026-10-22T10:00:00Z"},
        {"id":"q2","conversation_id":"c1","position":2,"text":"The header draws over the clock","created_at":"2026-10-22T10:00:00Z","attachments":[{"sha256":"$sha","name":"phone.png","media_type":"image/png","size":${png.size}}]}
    ]"""

    private fun open() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += request to String(request.body.toByteArray())

            when {
                path.endsWith("/attachments") && request.method == HttpMethod.Post ->
                    respond("""{"sha256":"$sha","name":"${request.url.parameters["name"]}","media_type":"image/png","size":${png.size}}""", HttpStatusCode.Created, json)
                path.startsWith("/api/v1/attachments/") -> respond(png, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "image/png"))
                path.contains("/queue/") -> respond("", HttpStatusCode.NoContent)
                path.endsWith("/queue") -> respond(queued, HttpStatusCode.OK, json)
                path.endsWith("/conversations") -> respond("[$WorkingJson]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(WorkingJson, HttpStatusCode.OK, json)
                path.endsWith("/events") -> respond(RunningTurn, HttpStatusCode.OK, json)
                path.endsWith("/nodes") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, QuietPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    private fun editSecond() {
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Actions for queued message 2").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Actions for queued message 2").performClick()
        rule.onNodeWithText("Edit").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Editing queued message 2").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun patches() = requests.filter { it.first.method == HttpMethod.Patch }.map { it.second }

    @Test
    fun editLoadsTheQueuedMessageWithItsImageIntoTheComposerAndHoldsIt() {
        open()

        editSecond()

        rule.onNodeWithText("Editing in the composer").assertIsDisplayed()
        rule.onNode(hasContentDescription("Message").and(hasText("The header draws over the clock", substring = true))).assertIsDisplayed()
        rule.onNode(hasContentDescription("Message").and(hasText("Image · phone.png", substring = true))).assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Queue message").fetchSemanticsNodes().size)
        assertEquals(listOf("""{"editing":true}"""), patches())
    }

    @Test
    fun saveSendsTheEditedTextAndImagesAndReleasesTheHold() {
        open()
        editSecond()

        rule.onNodeWithContentDescription("Message").performTextInput(" on the tablet too")
        rule.onNodeWithText("Save").performClick()
        rule.waitUntil(5_000) { patches().size == 2 }

        val saved = patches().last()

        assertEquals(true, saved.contains("on the tablet too") && saved.contains("\"editing\":false") && saved.contains(sha))
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Editing queued message 2").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun cancelReleasesTheHoldAndGivesBackTheDraftThePersonWasWriting() {
        open()
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Message").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Message").performTextInput("half written")

        editSecond()
        rule.onNodeWithText("Cancel").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Editing queued message 2").fetchSemanticsNodes().isEmpty() }

        rule.onNodeWithText("half written", substring = true).assertIsDisplayed()
        assertEquals(listOf("""{"editing":true}""", """{"editing":false}"""), patches())
    }

    @Test
    fun escapeCancelsTheEdit() {
        open()
        editSecond()

        rule.onNodeWithContentDescription("Message").performKeyInput { pressKey(Key.Escape) }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Editing queued message 2").fetchSemanticsNodes().isEmpty() }

        assertEquals(listOf("""{"editing":true}""", """{"editing":false}"""), patches())
    }
}
