package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import mycelium.api.StartProposal
import mycelium.api.StartProposalStep
import mycelium.api.ArtifactAccessRequest
import mycelium.api.MyceliumApi
import mycelium.state.AppState
import mycelium.state.MyceliumTool
import mycelium.state.MyceliumCall
import mycelium.state.ThreadItem
import mycelium.state.ToolStatus
import mycelium.state.Store
import mycelium.ui.ArtifactAccessControls
import mycelium.ui.InboxRequest
import mycelium.ui.LocalArtifactAccessControls
import mycelium.ui.LocalStartProposals
import mycelium.ui.StartProposals
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.RequestCard
import io.ktor.client.engine.mock.MockEngine
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class InboxMarkdownCardsTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val platform = AppsPlatform()

    private fun show(parsed: MyceliumCall, state: AppState = AppState(), proposal: StartProposal? = null) {
        val store = Store(MyceliumApi("https://mycelium.test", "token", MockEngine { json("[]") }), scopes.create())
        val call = ThreadItem.ToolCall("k1", "t1", "Request", "other", "", ToolStatus.Done, null, null)
        val starts = StartProposals(proposal?.let { mapOf(it.id to it) }.orEmpty(), {}, { _, _ -> }, { _, _ -> }, {})
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalStartProposals provides starts, LocalPlatform provides platform, LocalArtifactAccessControls provides ArtifactAccessControls(store, state)) {
                    RequestCard(store, state, "c1", InboxRequest.Mycelium(call, parsed), {})
                }
            }
        }
    }

    @Test
    fun proposedMessagesAndReasonsRenderInlineWithoutChangingTheirThreeLineLimit() {
        val message = "**Read** [the docs](https://example.com/docs) " + "before proceeding ".repeat(100)
        show(MyceliumCall.SendToConversation("Release notes", message, "**Check** the `plan`"))

        val nodes = rule.onAllNodesWithText("Read the docs " + "before proceeding ".repeat(100)).fetchSemanticsNodes()
        val layouts = mutableListOf<androidx.compose.ui.text.TextLayoutResult>()
        nodes.single().config[androidx.compose.ui.semantics.SemanticsActions.GetTextLayoutResult].action?.invoke(layouts)
        assertEquals(3, layouts.single().lineCount)
        rule.onNodeWithText("Check the plan").assertExists()
        rule.onNodeWithText(message).assertDoesNotExist()
    }

    @Test
    fun apiAccessReasonsRenderInlineLinks() {
        show(MyceliumCall.RequestApiAccess(emptyList(), 1, "**Read** [the docs](https://example.com/api)"))

        rule.onNodeWithText("Read the docs").performClick()
        assertEquals(listOf("https://example.com/api"), platform.opened)
    }

    @Test
    fun sshAccessReasonsRenderInlineLinks() {
        show(MyceliumCall.RequestSshAccess("atlas", 30, "**Read** [the docs](https://example.com/ssh)"))

        rule.onNodeWithText("Read the docs").performClick()
        assertEquals(listOf("https://example.com/ssh"), platform.opened)
    }

    @Test
    fun proposedStartPromptsRenderInlineLinksAndKeepTheirTitlePlain() {
        val proposal = StartProposal("p1", kind = "subagent", title = "Start review", steps = listOf(StartProposalStep("s1", "**Review**", firstMessage = "[the **docs**](https://example.com/start)")), state = "proposed", createdAt = "2026-11-01T10:00:00Z")
        show(MyceliumCall.StartProposed(MyceliumTool.StartSubagent, "Start review", "p1"), proposal = proposal)

        rule.onNodeWithText("the docs").performClick()
        assertEquals(listOf("https://example.com/start"), platform.opened)
        rule.onNodeWithText("**Review**").assertExists()
    }

    @Test
    fun artefactAccessReasonsRenderFullBlocksAndRetainApprovalControls() {
        val reason = "**Review** the plan\n\n- [x] Check notes\n\n```text\nsample command\n```"
        val request = ArtifactAccessRequest("r1", "a1", "c1", "view", reason, "requested", "2026-11-01T10:00:00Z")
        show(MyceliumCall.RequestArtifactAccess("a1", "view", reason, "r1"), AppState(artifactAccessRequests = mapOf("r1" to request)))

        rule.onNodeWithText("Review the plan").assertExists()
        rule.onNodeWithContentDescription("Done").assertExists()
        rule.onNodeWithContentDescription("Copy text code").assertExists()
        rule.onNodeWithText("Approve").assertExists()
        rule.onNodeWithText("Decline").assertExists()
    }
}
