package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import org.jetbrains.skia.EncodedImageFormat
import org.jetbrains.skia.Image
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.serializer
import mycelium.api.Event
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.state.buildThread
import mycelium.ui.LocalStartProposals
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import mycelium.ui.startProposals
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val ProposalId = "99999999-9999-9999-9999-999999999999"

private fun startSubagent(crossGroup: Boolean): List<Event> {
    val output = Json.encodeToString(serializer<String>(), "Waiting for the person to approve starting Review the schema${if (crossGroup) " in Shop" else ""} (proposal $ProposalId). Nothing has started.")
    val input = if (crossGroup) """{"group":"Shop","project":"ledger","title":"Review the schema","prompt":"Read the schema","reason":"Needed"}""" else """{"step":{"title":"Review the schema","prompt":"Read the schema","node":"laptop","agent":"claude","cwd":"/tmp"}}"""
    val tool = if (crossGroup) "start_conversation" else "start_subagent"

    return listOf(
        Event("c1", 1, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"mcp__mycelium__$tool","kind":"other","status":"pending","rawInput":{}}}""")),
        Event("c1", 2, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","rawInput":$input,"content":[{"type":"content","content":{"type":"text","text":$output}}]}}""")),
    )
}

private const val SingleChoice = ""","account":"a@example.com","model":null,"first_message":"Read the schema","node_choices":[],"account_choices":[]"""

private const val ModelChoices = ""","account":"a@example.com","model":"opus","first_message":"Read the schema","node_choices":[],"account_choices":[],"model_choices":[{"id":"opus","name":"Opus 5.5"},{"id":"sonnet","name":"Sonnet 5.5"}]"""

private const val Ambiguous = ""","first_message":"Read the schema","node_choices":[],"account_choices":["a@example.com","b@example.com"]"""

private fun proposal(state: String, step: String = SingleChoice, crossGroup: Boolean = false) =
    """{"id":"$ProposalId","conversation_id":"c1","kind":"${if (crossGroup) "conversation" else "subagent"}"${if (crossGroup) ",\"source_group\":\"Ops\",\"target_group\":\"Shop\"" else ""},"title":"Review the schema","steps":[{"key":"subagent","title":"Review the schema","depends_on":[],"gate":false,"node":{"id":"n1","name":"laptop","online":true},"folder":"/tmp","agent":"claude"${if (crossGroup) ",\"project\":\"ledger\"" else ""}$step}],"state":"$state","connect":false,"workflow_id":null,"created_at":"2026-10-23T10:00:00Z"}"""

private fun workflowReachingShop(state: String) =
    """{"id":"$ProposalId","conversation_id":"c1","kind":"workflow","source_group":"Ops","target_group":"Shop","title":"Reconcile October stock","steps":[""" +
        """{"key":"collect","title":"Collect the delivery log","depends_on":[],"gate":false,"node":{"id":"n1","name":"atlas","online":true},"folder":"ops/deliveries","agent":"claude","account":"a@example.com","first_message":"Export the October delivery log","node_choices":[],"account_choices":[]},""" +
        """{"key":"compare","title":"Compare with the ledger","depends_on":["collect"],"gate":false,"node":{"id":"n1","name":"atlas","online":true},"folder":"ledger","project":"ledger","agent":"claude","account":"a@example.com","first_message":"Compare the delivery log with the ledger","node_choices":[],"account_choices":[],"group":"Shop"}""" +
        """],"state":"$state","connect":false,"workflow_id":null,"created_at":"2026-10-23T10:00:00Z"}"""

class StartProposalCardTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val requests = java.util.concurrent.CopyOnWriteArrayList<Pair<HttpMethod, String>>()

    private val bodies = java.util.concurrent.ConcurrentHashMap<String, String>()

    private fun show(step: String = SingleChoice, crossGroup: Boolean = false, proposalIn: (String) -> String = { state -> proposal(state, step, crossGroup) }) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            requests += request.method to path
            bodies[path] = request.body.toByteArray().decodeToString()
            val body = when {
                path.endsWith("/connect") || path.endsWith("/approve") -> proposalIn("started")
                path.endsWith("/decline") -> proposalIn("declined")
                path.endsWith("/start-proposals") -> "[${proposalIn("proposed")}]"
                path.endsWith("/conversation-connections") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        val items = buildThread(startSubagent(crossGroup))

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                CompositionLocalProvider(LocalStartProposals provides startProposals(store, state)) {
                    Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}, runsOnModel = false) }
                }
            }
        }
    }

    private fun waitForText(text: String) {
        rule.waitUntil(timeoutMillis = 5_000) { rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun waitForRequest(method: HttpMethod, pathEnd: String) {
        rule.waitUntil(timeoutMillis = 5_000) { requests.toList().any { it.first == method && it.second.endsWith(pathEnd) } }
    }

    @Test
    fun theCardShowsTheNodeFolderAgentAccountAndFirstMessage() {
        show()

        waitForText("Create")
        rule.onNodeWithText("Node laptop, online").assertExists()
        rule.onNodeWithText("Folder /tmp").assertExists()
        rule.onNodeWithText("claude · a@example.com").assertExists()
        rule.onNodeWithText("Read the schema").assertExists()
    }

    @Test
    fun pressingCreateApprovesTheProposalAndTheCardShowsItStarted() {
        show()
        waitForText("Create")

        rule.onNodeWithText("Create").performClick()

        waitForRequest(HttpMethod.Post, "/start-proposals/$ProposalId/approve")
        waitForText("Started")
        rule.onNodeWithText("Don't create").assertDoesNotExist()
    }

    @Test
    fun pressingCreateAndConnectConnectsTheProposal() {
        show()
        waitForText("Create")

        rule.onNodeWithContentDescription("More ways to create").performClick()
        rule.onNodeWithText("Create and connect").performClick()

        waitForRequest(HttpMethod.Post, "/start-proposals/$ProposalId/connect")
    }

    @Test
    fun pressingDontCreateDeclinesAndTheCardShowsNothingWasCreated() {
        show()
        waitForText("Don't create")

        rule.onNodeWithText("Don't create").performClick()

        waitForRequest(HttpMethod.Post, "/start-proposals/$ProposalId/decline")
        waitForText("Not created")
        assertEquals(false, requests.any { it.second.endsWith("/approve") })
    }

    @Test
    fun anAccountStillToChooseBlocksCreateWithAnErrorUntilOneIsChosen() {
        show(Ambiguous)
        waitForText("Create")

        rule.onNodeWithText("Create").performClick()

        waitForText("Choose an account for Review the schema")
        assertEquals(false, requests.any { it.second.endsWith("/approve") })

        rule.onNodeWithText("b@example.com").performClick()
        rule.onNodeWithText("Create").performClick()

        waitForRequest(HttpMethod.Post, "/start-proposals/$ProposalId/approve")
    }

    @Test
    fun theCardStartsOnTheProposedModelUnlessAnotherIsChosen() {
        show(ModelChoices)
        waitForText("Create")

        rule.onNodeWithText("claude · a@example.com").assertExists()
        rule.onNodeWithContentDescription("Model: Opus 5.5").performClick()
        rule.onNodeWithContentDescription("Claude Code: Sonnet 5.5").performClick()
        rule.onNodeWithContentDescription("Model: Sonnet 5.5").assertExists()
        rule.onNodeWithText("Create").performClick()

        waitForRequest(HttpMethod.Post, "/start-proposals/$ProposalId/approve")
        assertEquals("""{"choices":{"subagent":{"model":"sonnet"}}}""", bodies.entries.single { it.key.endsWith("/start-proposals/$ProposalId/approve") }.value)
    }

    @Test
    fun aCardWithoutModelChoicesNamesTheModelAndOffersNoPicker() {
        show(SingleChoice.replace("\"model\":null", "\"model\":\"opus\""))
        waitForText("Create")

        rule.onNodeWithText("claude · a@example.com · opus").assertExists()
        rule.onNodeWithContentDescription("Model: opus").assertDoesNotExist()
    }

    @Test
    fun aCrossGroupCardNamesBothGroupsTheTargetAndTheTwoHintsAndCreateApproves() {
        show(crossGroup = true)

        waitForText("Create")
        rule.onNodeWithText("Proposed a conversation: Review the schema").assertExists()
        rule.onNodeWithText("From Ops").assertExists()
        rule.onNodeWithText("In Shop, project ledger").assertExists()
        rule.onNodeWithText("Folder /tmp").assertDoesNotExist()
        rule.onNodeWithText("Creating starts only this conversation and does not connect Ops and Shop.").assertExists()
        rule.onNodeWithText("Connecting allows messages in both directions between these two conversations until you revoke it in Settings.").assertExists()

        val png = Image.makeFromBitmap(rule.onRoot().captureToImage().asSkiaBitmap()).encodeToData(EncodedImageFormat.PNG)!!.bytes
        val out = java.io.File("build/screens/cross-group-start-proposal.png")
        out.parentFile.mkdirs()
        out.writeBytes(png)

        rule.onNodeWithText("Create").performClick()

        waitForRequest(HttpMethod.Post, "/start-proposals/$ProposalId/approve")
    }

    @Test
    fun aWorkflowCardNamesTheGroupOnlyOnTheStepOutsideTheCallersGroup() {
        show(proposalIn = ::workflowReachingShop)

        waitForText("Create")
        rule.onNodeWithText("Folder ops/deliveries").assertExists()
        rule.onNodeWithText("In Shop, project ledger").assertExists()
        assertEquals(1, rule.onAllNodesWithText("From Ops").fetchSemanticsNodes().size)
        rule.onNodeWithText("Creating starts only this workflow and does not connect Ops and Shop.").assertExists()
        rule.onNodeWithText("Connecting allows messages in both directions between these two conversations until you revoke it in Settings.").assertDoesNotExist()

        val png = Image.makeFromBitmap(rule.onRoot().captureToImage().asSkiaBitmap()).encodeToData(EncodedImageFormat.PNG)!!.bytes
        val out = java.io.File("build/screens/workflow-reaching-another-group.png")
        out.parentFile.mkdirs()
        out.writeBytes(png)
    }

    @Test
    fun aSameGroupCardShowsNoGroupLines() {
        show()

        waitForText("Create")
        rule.onNodeWithText("Folder /tmp").assertExists()
        assertEquals(0, rule.onAllNodesWithText("From Ops", substring = true).fetchSemanticsNodes().size)
    }
}
