package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.pressKey
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.PickedFile
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun draftsConversation(id: String, title: String) = """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"$title","status":"review",
"acp_session_id":"s","mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":null}"""

private class DraftFilePlatform : Platform by QuietPlatform {
    override fun pickFiles(onPicked: (List<PickedFile>) -> Unit) = onPicked(listOf(PickedFile("report.pdf", "application/pdf", ByteArray(2048) { 1 })))
}

class ComposerDraftsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val prompts = CopyOnWriteArrayList<String>()

    private fun open() {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val alpha = draftsConversation("c1", "Alpha chat")
        val bravo = draftsConversation("c2", "Bravo chat")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                path.endsWith("/attachments") -> respond("""{"sha256":"${"a".repeat(64)}","name":"report.pdf","media_type":"application/pdf","size":2048}""", HttpStatusCode.Created, json)
                path.endsWith("/prompt") -> {
                    prompts += path
                    respond("", HttpStatusCode.NoContent)
                }
                path.endsWith("/conversations") -> respond("[$alpha,$bravo]", HttpStatusCode.OK, json)
                path.endsWith("/conversations/c1") -> respond(alpha, HttpStatusCode.OK, json)
                path.endsWith("/conversations/c2") -> respond(bravo, HttpStatusCode.OK, json)
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, DraftFilePlatform(), Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) } }
    }

    private fun switchTo(title: String) {
        rule.onAllNodesWithText(title).onFirst().performClick()
    }

    @Test
    fun switchingChatsKeepsEachComposerTextAndAttachmentAndSendingClearsOnlyThatChat() {
        open()
        val field = rule.onNode(hasSetTextAction())

        field.performTextInput("half written")
        rule.onNodeWithContentDescription("Attach files").performClick()
        switchTo("Bravo chat")

        assertEquals("", field.fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
        assertEquals(0, rule.onAllNodesWithContentDescription("Remove attachment: report.pdf").fetchSemanticsNodes().size)

        switchTo("Alpha chat")

        assertEquals("half written", field.fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
        rule.onNodeWithContentDescription("Remove attachment: report.pdf").assertExists()

        field.performClick()
        field.performKeyInput { pressKey(Key.Enter) }
        rule.waitUntil(5_000) { field.fetchSemanticsNode().config[SemanticsProperties.EditableText].text.isEmpty() }
        switchTo("Bravo chat")
        field.performTextInput("other draft")
        switchTo("Alpha chat")

        assertEquals("", field.fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
        assertEquals(0, rule.onAllNodesWithContentDescription("Remove attachment: report.pdf").fetchSemanticsNodes().size)

        switchTo("Bravo chat")

        assertEquals("other draft", field.fetchSemanticsNode().config[SemanticsProperties.EditableText].text)
    }
}
