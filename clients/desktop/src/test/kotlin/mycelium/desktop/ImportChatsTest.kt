package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
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

private class ExportPlatform(private val picked: List<PickedFile>) : Platform by QuietPlatform {
    override fun pickFiles(onPicked: (List<PickedFile>) -> Unit) = onPicked(picked)
}

class ImportChatsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val imports = CopyOnWriteArrayList<Pair<HttpRequestData, String>>()

    private fun open(picked: List<PickedFile>, status: HttpStatusCode, reply: String) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            when {
                request.method == HttpMethod.Post && path.endsWith("/chats/import") -> {
                    imports += request to String(request.body.toByteArray())
                    respond(reply, status, json)
                }
                path.endsWith("/model-providers") || path.endsWith("/conversations") || path.endsWith("/nodes") -> respond("[]", HttpStatusCode.OK, json)
                else -> respond("{}", HttpStatusCode.OK, json)
            }
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, ExportPlatform(picked), Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Settings) } }
        rule.onNodeWithText("Import").performClick()
    }

    @Test
    fun importingAnExportSendsTheFileAndSaysHowManyChatsWereAddedAndHowManyWereAlreadyHere() {
        open(listOf(PickedFile("conversations.json", "application/json", "[]".toByteArray())), HttpStatusCode.OK, """{"imported":12,"skipped":3}""")

        rule.onNodeWithText("Import from Claude").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Imported 12 chats, 3 were already here").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Imported 12 chats, 3 were already here").assertIsDisplayed()
        assertEquals("[]", imports.single().second)
    }

    @Test
    fun aFileThatIsNotAnExportShowsTheServersAdviceNextToTheButton() {
        val message = "The file is not a Claude export. Choose the conversations.json from your Claude data export."
        open(listOf(PickedFile("notes.txt", "text/plain", "hello".toByteArray())), HttpStatusCode.BadRequest, """{"error":"not_claude_export","message":"$message"}""")

        rule.onNodeWithText("Import from Claude").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText(message).fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText(message).assertIsDisplayed()
    }

    @Test
    fun cancellingThePickerSendsNothing() {
        open(emptyList(), HttpStatusCode.OK, """{"imported":0,"skipped":0}""")

        rule.onNodeWithText("Import from Claude").performClick()

        assertEquals(0, imports.size)
    }
}
