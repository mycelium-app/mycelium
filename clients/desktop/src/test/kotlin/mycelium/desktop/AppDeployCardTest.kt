package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import kotlinx.serialization.json.Json
import mycelium.api.DeploymentProgress
import mycelium.api.DeploymentStep
import mycelium.api.Event
import mycelium.state.buildThread
import mycelium.ui.LocalAppDeployments
import mycelium.ui.LocalAppOpener
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private fun update(seq: Long, body: String) = Event("c1", seq, "acp_update", Json.parseToJsonElement("""{"update":$body}"""))

private fun deployThread(tool: String = "mcp__coolify__deploy") = listOf(
    update(1, """{"sessionUpdate":"tool_call","toolCallId":"call_1","title":"$tool","kind":"other","status":"pending","rawInput":{"app":"harbour-status"}}"""),
    update(2, """{"sessionUpdate":"tool_call_update","toolCallId":"call_1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"{\"app_uuid\":\"a1\",\"app_name\":\"harbour-status\",\"deployment_uuid\":\"d1\",\"address\":\"https://harbour-status.apps.example.com\"}"}}]}"""),
)

private val Steps = listOf(
    DeploymentStep("Created in Mycelium apps on atlas-box", "done", 2),
    DeploymentStep("Cloned main at 4f2c1a9", "done", 3),
    DeploymentStep("Building the image", "running", null),
    DeploymentStep("Start and health check", "pending", null),
)

class AppDeployCardTest {
    @get:Rule
    val rule = createComposeRule()

    private val platform = AppsPlatform()
    private val opened = mutableListOf<Pair<String, String?>>()
    private var polls = 0

    private fun show(tool: String = "mcp__coolify__deploy", fetch: suspend (String) -> DeploymentProgress) {
        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalAppDeployments provides { uuid -> polls += 1; fetch(uuid) }, LocalAppOpener provides { app, deployment -> opened += app to deployment }, LocalPlatform provides platform) {
                    Box(Modifier.width(640.dp).padding(16.dp)) {
                        ThreadView(buildThread(deployThread(tool)), onAnswer = { _, _ -> }, onStopWorkflow = {}, agent = "claude")
                    }
                }
            }
        }
    }

    @Test
    fun theCardShowsTheOrderedStepsWithDurationsWhileDeploying() {
        show { DeploymentProgress("d1", "a1", "harbour-status", "deploying", Steps) }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Deploying harbour-status").fetchSemanticsNodes().isNotEmpty() }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Building the image").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Cloned main at 4f2c1a9").assertExists()
        rule.onNodeWithText("3s").assertExists()
        rule.onNodeWithText("Build log").assertExists()
    }

    @Test
    fun theCardKeepsPollingUntilTheDeploymentFinishesThenShowsTheAddress() {
        show { if (polls < 2) DeploymentProgress("d1", "a1", "harbour-status", "deploying", Steps) else DeploymentProgress("d1", "a1", "harbour-status", "deployed", Steps.map { it.copy(state = "done") }, address = "https://harbour-status.apps.example.com") }

        rule.waitUntil(10_000) { rule.onAllNodesWithText("Deployed at harbour-status.apps.example.com").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Open app").performClick()
        assertEquals(listOf("https://harbour-status.apps.example.com"), platform.opened)
    }

    @Test
    fun theBuildLogButtonOpensTheAppOnThatDeployment() {
        show { DeploymentProgress("d1", "a1", "harbour-status", "deploying", Steps) }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Build log").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Build log").performClick()

        assertEquals(listOf<Pair<String, String?>>("a1" to "d1"), opened)
    }

    @Test
    fun aFailedDeploymentShowsTheLastLogLines() {
        show { DeploymentProgress("d1", "a1", "harbour-status", "failed", Steps.map { if (it.state == "running") it.copy(state = "failed") else it }, logTail = listOf("npm ERR! missing script: build")) }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("npm ERR! missing script: build").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun otherToolsKeepTheOrdinaryCard() {
        show("mcp__coolify__list_apps") { error("not polled") }

        rule.waitUntil(5_000) { rule.onAllNodesWithText("List apps").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, polls)
    }
}
