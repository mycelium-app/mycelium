package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import mycelium.api.MyceliumApi
import mycelium.api.parseStreamMessage
import mycelium.state.Store
import mycelium.ui.InstructionProposals
import mycelium.ui.LocalOpenConversation
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private fun proposal(id: String, text: String, outOfDate: Boolean = false, state: String = "proposed", note: String? = null) =
    """{"id":"$id","conversation_id":"c1","conversation_title":"Tidy the release notes","layer":"global","layer_name":"Global","base_revision":"abc","base_text":"# Rules\nBe brief.\nUse British English.\nKeep diffs small.\nNo comments.\nTest first.\nCommit small.","text":"$text","reason":"You said: never commit unless I ask.","state":"$state","note":${note?.let { "\"$it\"" }},"out_of_date":$outOfDate,"created_at":"2026-10-23T10:00:00Z"}"""

private const val Edited = "# Rules\\nBe brief.\\nUse British English.\\nKeep diffs small.\\nNever commit unasked.\\nTest first.\\nCommit small."

class InstructionProposalsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.Collections.synchronizedList(mutableListOf<Triple<HttpMethod, String, String>>())

    private fun requestTo(method: HttpMethod, pathEnd: String): Triple<HttpMethod, String, String> {
        rule.waitUntil(timeoutMillis = 5_000) { requests.toList().any { it.first == method && it.second.endsWith(pathEnd) } }

        return requests.toList().first { it.first == method && it.second.endsWith(pathEnd) }
    }

    private fun show(listed: String, opened: MutableList<String> = mutableListOf()): Store {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += Triple(request.method, path, request.body.toByteArray().decodeToString())
            val body = when {
                path.endsWith("/instruction-proposals") -> "[$listed]"
                path.contains("/instruction-proposals/") -> proposal("p1", Edited, state = if (path.endsWith("/approve")) "approved" else "declined")
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        rule.setContent { MyceliumTheme(true) { CompositionLocalProvider(LocalOpenConversation provides { opened += it }) { InstructionProposals(store) } } }

        return store
    }

    @Test
    fun aWaitingProposalShowsItsReasonAndAMarkedDiffOfOnlyTheChange() {
        show(proposal("p1", Edited))

        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithContentDescription("Proposed edit to the Global instructions").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("You said: never commit unless I ask.").assertExists()
        rule.onNodeWithContentDescription("Added: Never commit unasked.").assertExists()
        rule.onNodeWithContentDescription("Removed: No comments.").assertExists()
        rule.onNodeWithText("2 unchanged lines").assertExists()
    }

    @Test
    fun approvingPostsTheApprovalAndTheCardLeavesTheWaitingList() {
        show(proposal("p1", Edited))
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithContentDescription("Approve the proposed edit to the Global instructions").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Approve the proposed edit to the Global instructions").performClick()

        requestTo(HttpMethod.Post, "/instruction-proposals/p1/approve")
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithContentDescription("Proposed edit to the Global instructions").fetchSemanticsNodes().isEmpty() }
        rule.onNodeWithText("Show decided (1)").performClick()
        rule.onNodeWithText("Approved: Global instructions").assertExists()
    }

    @Test
    fun decidedProposalsAreHiddenUntilShowDecidedIsPressed() {
        show(proposal("p1", Edited, state = "approved") + "," + proposal("p2", Edited))

        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Show decided (1)").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithText("Approved: Global instructions").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Proposed edit to the Global instructions").assertExists()

        rule.onNodeWithText("Show decided (1)").performClick()

        rule.onNodeWithText("Approved: Global instructions").assertExists()
    }

    @Test
    fun decliningSendsTheOptionalNote() {
        show(proposal("p1", Edited))
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithContentDescription("Decline the proposed edit to the Global instructions").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Decline the proposed edit to the Global instructions").performClick()
        rule.onNodeWithContentDescription("Note to the agent (optional)").performTextInput("Too broad")
        rule.onNodeWithContentDescription("Decline the proposed edit to the Global instructions").performClick()

        val declined = requestTo(HttpMethod.Post, "/instruction-proposals/p1/decline")
        assertTrue(declined.third.contains("\"note\":\"Too broad\""), declined.third)
        assertTrue(!declined.third.contains("\"redo\":true"), declined.third)
    }

    @Test
    fun anOutOfDateProposalCannotBeApprovedAndOffersToAskTheAgentToRedoIt() {
        show(proposal("p1", Edited, outOfDate = true))
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText("Out of date").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(0, rule.onAllNodesWithContentDescription("Approve the proposed edit to the Global instructions").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Decline the out of date edit and ask the agent to propose it again").performClick()

        val redo = requestTo(HttpMethod.Post, "/instruction-proposals/p1/decline")
        assertTrue(redo.third.contains("\"redo\":true"), redo.third)
    }

    @Test
    fun aProposalStreamedWhileTheListIsOpenAppearsAndTheConversationOpensFromIt() {
        val opened = mutableListOf<String>()
        val store = show("", opened)

        store.receive(parseStreamMessage("""{"type":"instruction_proposal",${proposal("p2", Edited).removePrefix("{")}""")!!)

        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithContentDescription("Proposed edit to the Global instructions").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Open the conversation that proposed this edit").performClick()
        assertEquals(listOf("c1"), opened)
    }
}

class InstructionProposalsRenderTest {
    @get:Rule
    val scopes = TestScopes()
    @Test
    fun theProposalsListRendersWithAMarkedDiff() {
        val listed = "[${proposal("p1", Edited)},${proposal("p2", Edited, outOfDate = true)},${proposal("p3", Edited, state = "declined", note = "Too broad")}]"
        val store = Store(fakeApi(extra = mapOf("/instruction-proposals" to listed)), scopes.create())
        kotlinx.coroutines.runBlocking { store.refreshInstructionProposals() }

        for (phone in listOf(false, true)) {
            val file = renderAt("instruction-proposals", true, phone) { androidx.compose.foundation.layout.Box(androidx.compose.ui.Modifier.fillMaxSize().background(mycelium.ui.MyTheme.colors.bgContent).padding(16.dp)) { InstructionProposals(store) } }
            assertTrue(file.length() > 5_000)
        }
    }
}
