package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.state.Store
import mycelium.api.MyceliumApi
import mycelium.ui.LocalThreadSubagents
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadSubagents
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.time.Clock
import kotlin.time.Duration.Companion.minutes

private const val Parent = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Bump the UI kit","status":"working","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val Waiting = """{"id":"c2","owner":"u","node_id":"n1","agent":"codex","cwd":"/work/harbour","title":"Run the e2e suite on the new kit","status":"input","parent_id":"c1","side_kind":"subagent","acp_session_id":"s","created_at":"2026-10-22T10:01:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val Working = """{"id":"c3","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Check the changelog wording","model":"Sonnet 5.5","status":"working","parent_id":"c1","side_kind":"subagent","acp_session_id":"s","created_at":"2026-10-22T10:02:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val Finished = """{"id":"c4","owner":"u","node_id":"n1","agent":"codex","cwd":"/work/harbour","title":"Audit the lockfile for drift","status":"done","parent_id":"c1","side_kind":"subagent","acp_session_id":"s","created_at":"2026-10-22T10:03:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val Failed = """{"id":"c5","owner":"u","node_id":"n1","agent":"codex","cwd":"/work/harbour","title":"Dry run the migration","status":"failed","parent_id":"c1","side_kind":"subagent","acp_session_id":"s","created_at":"2026-10-22T10:03:30Z","updated_at":"2026-10-22T10:05:00Z"}"""
private const val ReadyToReview = """{"id":"c6","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Tidy the release notes","status":"review","parent_id":"c1","side_kind":"subagent","acp_session_id":"s","created_at":"2026-10-22T10:04:00Z","updated_at":"2026-10-22T10:05:00Z"}"""
private fun subagent(id: String, title: String, status: String, minutesAgo: Long): String {
    val at = Clock.System.now() - minutesAgo.minutes

    return """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"$title","status":"$status","parent_id":"c1","side_kind":"subagent","acp_session_id":"s","created_at":"$at","updated_at":"$at"}"""
}

private const val WaitingEvents = """[{"conversation_id":"c2","seq":1,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"31 of 48 specs pass. Should I install Chromium so the whole suite can run?"}}}}]"""

class SubagentReplyTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private val opened = java.util.concurrent.CopyOnWriteArrayList<String>()

    private fun show() {
        render("[$Parent,$Waiting,$Working]")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Reply to Run the e2e suite on the new kit").fetchSemanticsNodes().isNotEmpty() }
    }

    private fun render(conversations: String) {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = String(request.body.toByteArray())

            if (request.method == HttpMethod.Post) {
                posts += path to body
            }

            val response = when {
                path.endsWith("/conversations") -> conversations
                path.endsWith("/conversations/c2/events") -> WaitingEvents
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/workflows") -> "[]"
                else -> "{}"
            }

            respond(response, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                CompositionLocalProvider(LocalThreadSubagents provides ThreadSubagents(store, state, "c1", { opened += it })) {
                    Box(Modifier.width(720.dp)) { ThreadView(emptyList(), onAnswer = { _, _ -> }, onStopWorkflow = {}) }
                }
            }
        }
    }

    private fun row(title: String) = rule.onNode(hasContentDescription("$title, ", substring = true) and hasClickAction())

    private fun waitForRow(title: String) {
        rule.waitUntil(5_000) { rule.onAllNodes(hasContentDescription("$title, ", substring = true) and hasClickAction()).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun height(interaction: SemanticsNodeInteraction) = interaction.fetchSemanticsNode().boundsInRoot.height

    private fun top(title: String) = row(title).fetchSemanticsNode().boundsInRoot.top

    private fun titled(count: Int) = (1..count).map { "Check package $it" }

    private fun renderRunning(titles: List<String>) {
        render(titles.mapIndexed { index, title -> subagent("r$index", title, "working", index.toLong()) }.joinToString(",", "[$Parent,", "]"))
        waitForRow(titles.first())
    }

    @Test
    fun theCardListsRunningAndRecentlyFinishedSubagentsAndLeavesOutOlderOnes() {
        render("[$Parent,$Working,$Finished,$Failed,$ReadyToReview,${subagent("c7", "Format the upgrade notes", "done", 10)},${subagent("c8", "Compare the bundle sizes", "failed", 5)},${subagent("c9", "Read the release notes", "review", 20)}]")
        waitForRow("Check the changelog wording")

        rule.onNodeWithContentDescription("4 subagents, hide them").assertExists()
        row("Format the upgrade notes").assertExists()
        row("Compare the bundle sizes").assertExists()
        row("Read the release notes").assertExists()
        assertEquals(0, rule.onAllNodes(hasContentDescription("Audit the lockfile for drift, ", substring = true)).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(hasContentDescription("Dry run the migration, ", substring = true)).fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(hasContentDescription("Tidy the release notes, ", substring = true)).fetchSemanticsNodes().size)
    }

    @Test
    fun thereIsNoSubagentsCardWhenEverySubagentFinishedOverAnHourAgo() {
        render("[$Parent,$Finished,$Failed,$ReadyToReview]")
        rule.waitForIdle()

        assertEquals(0, rule.onAllNodesWithContentDescription("Subagents").fetchSemanticsNodes().size)
        assertEquals(0, rule.onAllNodes(hasContentDescription("subagents, hide them", substring = true)).fetchSemanticsNodes().size)
    }

    @Test
    fun theCardOrdersWaitingThenRunningThenMostRecentlyFinished() {
        render("[$Parent,${subagent("c7", "Format the upgrade notes", "done", 30)},${subagent("c8", "Compare the bundle sizes", "failed", 5)},$Working,$Waiting]")
        waitForRow("Run the e2e suite on the new kit")

        val tops = listOf("Run the e2e suite on the new kit", "Check the changelog wording", "Compare the bundle sizes", "Format the upgrade notes").map(::top)

        assertEquals(tops.sorted(), tops)
    }

    @Test
    fun aLongListScrollsInsideTheCardAtAFixedMaximumHeight() {
        val titles = titled(12)

        renderRunning(titles)

        val rowHeight = height(row(titles.first()))
        val cardHeight = height(rule.onNodeWithContentDescription("Subagents"))

        assertTrue(cardHeight > rowHeight * 5, "the card shows about five rows, was $cardHeight for rows of $rowHeight")
        assertTrue(cardHeight < rowHeight * 7, "the card stops growing, was $cardHeight for rows of $rowHeight")
        row(titles.last()).assertIsNotDisplayed()
        rule.captureScreen("2.60-subagents-long-list")

        row(titles.last()).performScrollTo()

        row(titles.last()).assertIsDisplayed()
        assertEquals(cardHeight, height(rule.onNodeWithContentDescription("Subagents")))
    }

    @Test
    fun focusReachesARowBelowTheFoldOfALongList() {
        val titles = titled(12)

        renderRunning(titles)

        row(titles.last()).requestFocus()
        rule.waitForIdle()

        row(titles.last()).assertIsDisplayed()
    }

    @Test
    fun aShortListKeepsItsOwnHeight() {
        val titles = titled(2)

        renderRunning(titles)

        assertTrue(height(rule.onNodeWithContentDescription("Subagents")) < height(row(titles.first())) * 4)
    }

    @Test
    fun aSubagentWaitingForAnAnswerShowsItsQuestionTheWaitingLineAndAReplyFieldInItsCard() {
        show()

        rule.onNodeWithContentDescription("Run the e2e suite on the new kit, waiting for your answer").assertExists()
        rule.onNodeWithText("31 of 48 specs pass. Should I install Chromium so the whole suite can run?").assertExists()
        rule.onNodeWithText("Waiting for your answer").assertExists()
        rule.onNodeWithText("Reply to this subagent").assertExists()
        rule.onNodeWithContentDescription("Send to Run the e2e suite on the new kit").assertExists()
        rule.captureScreen("2.26-subagent-reply")
    }

    @Test
    fun aSubagentRowNamesItsHarnessModelAndStatusAndOpensTheSubagent() {
        show()

        rule.onNode(hasContentDescription("Check the changelog wording, Claude, Sonnet 5.5, Working", substring = true)).assertExists()
        rule.onNode(hasContentDescription("Run the e2e suite on the new kit, Codex, Needs input", substring = true) and hasClickAction()).performClick()

        assertEquals(listOf("c2"), opened.toList())
    }

    @Test
    fun sendPostsTheReplyUnchangedToThatSubagentsPromptRoute() {
        show()

        rule.onNodeWithContentDescription("Reply to Run the e2e suite on the new kit").performTextInput("Yes, install it")
        rule.onNodeWithContentDescription("Send to Run the e2e suite on the new kit").performClick()
        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/conversations/c2/prompt") } }

        val body = posts.single { it.first.endsWith("/conversations/c2/prompt") }.second
        assertTrue(body.contains("\"text\":\"Yes, install it\""), body)
    }
}
