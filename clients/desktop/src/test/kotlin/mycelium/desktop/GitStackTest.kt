package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.UriHandler
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.OutgoingContent
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.serialization.decodeFromString
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.Store
import mycelium.ui.GitPanel
import mycelium.ui.MyceliumTheme
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.test.captureToImage
import org.jetbrains.skia.EncodedImageFormat
import org.jetbrains.skia.Image
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val StackConversationJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work","title":"Importer","status":"review","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"""

private fun stackJson(townAvailable: Boolean, inferred: Boolean = false, pullRequests: Boolean = true) = """{
  "current":"one","main":"main","inferred":$inferred,"git_town_available":$townAvailable,"pull_requests_available":$pullRequests,
  "branches":[
    {"name":"main","depth":0,"main":true,"has_upstream":true,"pushed":true,"subject":"Start the project"},
    {"name":"one","parent":"main","depth":1,"current":true,"ahead":2,"behind":0,"has_upstream":true,"pushed":true,"subject":"Add the importer",
     "pull_request":{"number":123,"label":"PR #123","title":"Add the importer","url":"https://forge.example/pull/123","state":"draft"}},
    {"name":"two","parent":"one","depth":2,"ahead":1,"behind":3,"has_upstream":false,"pushed":false,"needs_sync":true,"subject":"Wire the importer in"}
  ]}"""

private class StackFake(val stack: String, val townReply: Pair<HttpStatusCode, String> = HttpStatusCode.OK to "{}") {
    val requests: MutableList<HttpRequestData> = java.util.concurrent.CopyOnWriteArrayList()

    val api = MyceliumApi(
        "https://mycelium.test",
        "token",
        MockEngine { request ->
            requests += request
            val path = request.url.encodedPath
            val reply = when {
                path.endsWith("/git/stack") -> HttpStatusCode.OK to stack
                path.contains("/git/town/") -> townReply
                path.endsWith("/git/status") -> HttpStatusCode.OK to """{"branch":"one","ahead":0,"behind":0,"files":[]}"""
                path.endsWith("/git/log") -> HttpStatusCode.OK to "[]"
                else -> HttpStatusCode.OK to "{}"
            }

            respond(reply.second, reply.first, headersOf(HttpHeaders.ContentType, "application/json"))
        },
    )

    fun sent(route: String): Boolean = requests.any { it.url.encodedPath.endsWith(route) }

    fun bodyOf(route: String): String = (requests.last { it.url.encodedPath.endsWith(route) }.body as OutgoingContent.ByteArrayContent).bytes().decodeToString()
}

class GitStackTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val opened = CopyOnWriteArrayList<String>()

    private fun open(fake: StackFake) {
        render(fake)
        rule.onNodeWithText("Stack").performClick()
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Current branch").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun render(fake: StackFake) {
        val conversation = MyceliumJson.decodeFromString<Conversation>(StackConversationJson)
        val store = Store(fake.api, scopes.create())
        val handler = object : UriHandler {
            override fun openUri(uri: String) {
                opened += uri
            }
        }

        rule.setContent {
            CompositionLocalProvider(LocalUriHandler provides handler) {
                MyceliumTheme(true) {
                    Box(Modifier.width(360.dp).height(640.dp)) { GitPanel(store, conversation, Modifier.fillMaxSize()) }
                }
            }
        }
    }

    @Test
    fun theStackListsEveryBranchWithTheCurrentOneMarkedInTextAndTheirTags() {
        open(StackFake(stackJson(townAvailable = true)))

        rule.onNodeWithText("main").assertIsDisplayed()
        rule.onNodeWithText("Current branch").assertIsDisplayed()
        rule.onNodeWithText("Needs sync").assertIsDisplayed()
        rule.onNodeWithText("Not pushed").assertIsDisplayed()
        rule.onNodeWithText("+2 \u22120").assertIsDisplayed()
        rule.onAllNodesWithText("Add the importer")[0].assertIsDisplayed()
        rule.onNodeWithText("+1 \u22123").assertIsDisplayed()
        rule.onNodeWithText("Wire the importer in").assertIsDisplayed()
    }

    @Test
    fun aBranchesPullRequestShowsItsNumberAndStateAndTheLinkOpensItInTheBrowser() {
        open(StackFake(stackJson(townAvailable = true)))

        rule.onNodeWithText("PR #123 draft").assertIsDisplayed()
        rule.onNodeWithContentDescription("Open pull request 123: Add the importer").performClick()

        assertEquals(listOf("https://forge.example/pull/123"), opened)
    }

    @Test
    fun aBranchWithoutAPullRequestSaysSoAndSwitchingToItGoesThroughGit() {
        val fake = StackFake(stackJson(townAvailable = true))
        open(fake)

        rule.onNodeWithText("No pull request").assertIsDisplayed()
        rule.onNodeWithContentDescription("Switch to two").performClick()
        rule.waitUntil(5_000) { fake.sent("/git/switch") }

        assertEquals("""{"branch":"two"}""", fake.bodyOf("/git/switch"))
    }

    @Test
    fun withoutGitTownTheActionsAreHidden() {
        open(StackFake(stackJson(townAvailable = false, inferred = true)))

        assertEquals(0, rule.onAllNodesWithText("Sync stack").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodesWithText("Propose").fetchSemanticsNodes().size)
        rule.onNodeWithText("two").assertIsDisplayed()
    }

    @Test
    fun syncingTheStackRunsGitTownAndShowsItsOwnOutputWhenItFails() {
        val fake = StackFake(stackJson(townAvailable = true), HttpStatusCode.Conflict to """{"error":"git_failed","message":"conflict in importer.kt"}""")
        open(fake)

        rule.onNodeWithText("Sync stack").performClick()
        rule.waitUntil(5_000) { fake.sent("/git/town/sync") }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("conflict in importer.kt", substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun proposingIsOfferedOnlyForTheCurrentBranchWhenItHasNoPullRequest() {
        val branchless = stackJson(townAvailable = true).replace(Regex(""",\s*"pull_request":\{[^}]*\}"""), "")
        val fake = StackFake(branchless)
        open(fake)

        rule.onNodeWithContentDescription("Propose one").performClick()
        rule.waitUntil(5_000) { fake.sent("/git/town/propose") }

        assertTrue(rule.onAllNodes(hasContentDescription("Propose two")).fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun walkthroughAsksTheAgentToWalkThroughTheUncommittedChanges() {
        val fake = StackFake(stackJson(townAvailable = true))
        render(fake)

        rule.onNodeWithContentDescription("More ways to view diff").performClick()
        rule.onNodeWithText("Walk me through changes").performClick()
        rule.waitUntil(5_000) { fake.sent("/conversations/c1/prompt") }

        assertTrue(fake.bodyOf("/conversations/c1/prompt").contains("the uncommitted changes in the working tree"))
    }

    @Test
    fun theChangesRowRendersForComparisonWithTheArtboard() {
        render(StackFake(stackJson(townAvailable = true)))
        rule.waitUntil(5_000) { rule.onAllNodesWithText("View diff").fetchSemanticsNodes().isNotEmpty() }
        val png = Image.makeFromBitmap(rule.onRoot().captureToImage().asSkiaBitmap()).encodeToData(EncodedImageFormat.PNG)!!.bytes
        val out = java.io.File("build/screens/git-changes-walkthrough-dark.png")
        out.parentFile.mkdirs()
        out.writeBytes(png)

        assertTrue(out.length() > 5_000)
    }
}
