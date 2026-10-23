package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.OutgoingContent
import io.ktor.http.headersOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.serialization.decodeFromString
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.Store
import mycelium.ui.GitPanel
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Now = "2026-01-01T00:00:00Z"

private fun graphCommit(hash: String, parents: String, subject: String, refs: String = "") =
    """{"hash":"$hash","parents":[$parents],"author":"Sam","date":"$Now","refs":[$refs],"subject":"$subject"}"""

private val HistoryJson = listOf(
    graphCommit("aaaaaaa1", "\"bbbbbbb2\",\"ccccccc3\"", "Merge the importer", """{"name":"main","kind":"branch","current":true},{"name":"origin/main","kind":"remote"},{"name":"v1.2","kind":"tag"}"""),
    graphCommit("bbbbbbb2", "\"ddddddd4\"", "Tidy the settings screen"),
    graphCommit("ccccccc3", "\"ddddddd4\"", "Add the importer", """{"name":"feature/importer","kind":"branch"}"""),
    graphCommit("eeeeeee5", "\"ddddddd4\"", "Try a spike", """{"name":"spike","kind":"branch"}"""),
    graphCommit("ddddddd4", "\"fffffff6\"", "Start the project"),
    graphCommit("fffffff6", "", "Initial commit"),
).joinToString(",", "[", "]")

private const val BranchesJson = """{"current":"main","local":[
    {"name":"main","author":"Sam","date":"$Now","subject":"Merge the importer"},
    {"name":"feature/importer","author":"Alex","date":"$Now","subject":"Add the importer"}],
    "remote":[{"name":"origin/main","author":"Sam","date":"$Now","subject":"Merge the importer"},
    {"name":"origin/release","author":"Kim","date":"$Now","subject":"Cut the release"}]}"""

private const val ShowJson = """{"diff":"diff --git a/src/importer.kt b/src/importer.kt\n--- a/src/importer.kt\n+++ b/src/importer.kt\n@@ -1 +1 @@\n-old\n+new\n","truncated":false}"""

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work","title":"Importer","status":"review","created_at":"$Now","updated_at":"$Now"}"""

private class GitFake {
    val requests = mutableListOf<HttpRequestData>()

    val api = MyceliumApi(
        "https://mycelium.test",
        "token",
        MockEngine { request ->
            requests += request
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/git/status") -> """{"branch":"main","ahead":0,"behind":0,"files":[]}"""
                path.endsWith("/git/graph") -> if (request.url.parameters["skip"] == "0") HistoryJson else "[]"
                path.endsWith("/git/show") -> ShowJson
                path.endsWith("/git/branches") -> BranchesJson
                path.endsWith("/git/log") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        },
    )

    fun switchBody(): String = (requests.last { it.url.encodedPath.endsWith("/git/switch") }.body as OutgoingContent.ByteArrayContent).bytes().decodeToString()
}

class GitGraphTest {
    @get:Rule
    val rule = createComposeRule()

    private fun open(fake: GitFake) {
        val conversation = MyceliumJson.decodeFromString<Conversation>(ConversationJson)
        val store = Store(fake.api, CoroutineScope(Dispatchers.Unconfined))

        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(360.dp).height(640.dp)) { GitPanel(store, conversation, Modifier.fillMaxSize()) }
            }
        }
    }

    private fun awaitText(description: String) {
        rule.waitUntil(5_000) { rule.onAllNodes(hasContentDescription(description, substring = true)).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theGraphTabListsCommitsWithTheirRefsAndSelectingOneOpensItsDiff() {
        open(GitFake())

        rule.onNodeWithText("Graph").performClick()
        awaitText("Merge the importer")

        rule.onNode(hasContentDescription("Merge the importer, Sam, ", substring = true) and hasContentDescription("tag v1.2", substring = true)).assertIsDisplayed()
        rule.onNode(hasContentDescription("Add the importer", substring = true) and hasContentDescription("branch feature/importer", substring = true)).assertIsDisplayed()
        rule.onNode(hasContentDescription("Merge the importer", substring = true)).performClick()
        rule.waitForIdle()

        awaitText("Back to the graph")
        rule.onNodeWithContentDescription("Back to the graph").assertIsDisplayed()
        awaitText("Edit to src")
        rule.onNodeWithContentDescription("Edit to src/importer.kt, 1 added, 1 removed").assertIsDisplayed()
    }

    @Test
    fun theBranchPickerListsLocalAndRemoteBranchesAndSwitchesToAnotherOne() {
        val fake = GitFake()
        open(fake)

        rule.onNodeWithContentDescription("Switch branch, current branch main").performClick()
        awaitText("origin/release")

        rule.onNodeWithText("Local branches").assertIsDisplayed()
        rule.onNodeWithText("Remote branches").assertIsDisplayed()
        rule.onNodeWithContentDescription("main, current branch").assertIsDisplayed()
        rule.onNodeWithContentDescription("origin/release, remote branch").performClick()
        rule.waitForIdle()

        assertEquals("""{"branch":"release","create":true,"track":"origin/release"}""", fake.switchBody())
    }

    @Test
    fun typingANewBranchNameOffersToCreateItAndEnterCreatesIt() {
        val fake = GitFake()
        open(fake)

        rule.onNodeWithContentDescription("Switch branch, current branch main").performClick()
        awaitText("origin/release")
        rule.onNodeWithContentDescription("Search branches").performTextInput("fresh-idea")
        rule.waitForIdle()

        rule.onNodeWithContentDescription("Create branch fresh-idea").assertIsDisplayed()
        rule.onNodeWithContentDescription("Create branch fresh-idea").performClick()
        rule.waitForIdle()

        assertEquals("""{"branch":"fresh-idea","create":true}""", fake.switchBody())
        assertTrue(fake.requests.any { it.method == HttpMethod.Post })
    }

    @Test
    fun rendersTheGraphForReview() {
        val fake = GitFake()
        open(fake)
        rule.onNodeWithText("Graph").performClick()
        awaitText("Merge the importer")
        File("build/screens").mkdirs()
        File("build/screens/git-graph.png").writeBytes(rule.onRoot().captureToImage().asSkiaBitmap().let { bitmap -> org.jetbrains.skia.Image.makeFromBitmap(bitmap).encodeToData()!!.bytes })
    }
}
