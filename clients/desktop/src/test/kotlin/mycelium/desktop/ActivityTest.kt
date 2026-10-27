package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasText
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import mycelium.Connection
import mycelium.DeviceInfo
import mycelium.Platform
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val Nodes = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.1.10","online":true,"draining":false,"agents":[],"logins":[]},
{"id":"n2","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"v0.1.10","online":true,"draining":false,"agents":[],"logins":[]}]"""

private const val Conversations = """[{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/backups","title":"Backups","status":"done","acp_session_id":"s","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}]"""

private fun row(key: String, name: String, turns: Int, reported: Int, input: Long, cost: String, workingMs: Long) =
    """{"key":"$key","name":"$name","turns":$turns,"reported_turns":$reported,"working_ms":$workingMs,"input_tokens":$input,"output_tokens":0,"cache_read_tokens":0,"cache_write_tokens":0,"cost_usd":$cost}"""

private val Reported = """{"range":"7d","totals":${row("", "", 4, 3, 182_000_000, "1940.2", 11_520_000)},
"hours":[${row("2026-10-02T09", "2026-10-02T09", 1, 1, 1_000_000, "10.0", 60_000)},${row("2026-10-02T10", "2026-10-02T10", 3, 2, 181_000_000, "1930.2", 11_460_000)}],
"days":[${row("2026-10-01", "2026-10-01", 1, 1, 1_000_000, "10.0", 60_000)},${row("2026-10-02", "2026-10-02", 3, 2, 181_000_000, "1930.2", 11_460_000)}],
"conversations":[${row("c1", "Backups", 4, 3, 182_000_000, "1940.2", 11_520_000)}],
"nodes":[${row("n1", "willow", 4, 3, 182_000_000, "1940.2", 11_520_000)}],
"agents":[${row("claude", "claude", 3, 3, 182_000_000, "1940.2", 11_000_000)},${row("codex", "codex", 1, 0, 0, "null", 520_000)}],
"projects":[${row("p1", "midden", 4, 3, 182_000_000, "1940.2", 11_520_000)}],
"models":[${row("claude-opus-5-5", "claude-opus-5-5", 3, 3, 182_000_000, "1940.2", 11_000_000)}],
"unreported_agents":["codex"]}"""

private const val Empty = """{"range":"7d","totals":{"key":"","name":""},"unreported_agents":[]}"""

private object ActivityPlatform : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): Connection? = null
    override fun saveConnection(connection: Connection?) = Unit
    override fun device() = DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
}

class ActivityTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val asked = CopyOnWriteArrayList<String>()

    private fun shown(text: String): Boolean = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun showSidebar(answer: String, width: Dp = 1400.dp) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/activity") -> {
                    asked += request.url.encodedQuery
                    answer
                }
                path.endsWith("/nodes") -> Nodes
                path.endsWith("/conversations") -> Conversations
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Box(Modifier.width(width)) { Shell(store, ActivityPlatform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Nodes) } } }
        rule.onNodeWithContentDescription("Activity").performClick()
        rule.waitForIdle()
    }

    @Test
    fun theActivityButtonOpensTheSevenDayTotalsBreakdownsAndChartSummary() {
        showSidebar(Reported)

        rule.onNodeWithText("4 turns, 3h 12m working").assertExists()
        rule.onNodeWithContentDescription("Tokens per hour, last 7 days. Peak 181M in the hour from 2 Oct 10:00, 182M in total.").assertExists()
        assertEquals(true, shown("~$1,940"))
        assertEquals(true, shown("3h 12m"))
        assertEquals(true, shown("182M"))
        rule.onNodeWithText("Peak 181M in the hour from 2 Oct 10:00, 182M in total.").assertExists()
        assertEquals(true, shown("midden"))
        assertEquals(true, shown("claude-opus-5-5"))
        assertEquals("range=7d", asked.last())
    }

    @Test
    fun energyWaterAndForestAreShownAsEstimatesWithTheirSourcesNamed() {
        showSidebar(Reported)

        rule.onNodeWithText("Energy and water").assertExists()
        rule.onNodeWithText("~36 kWh").assertExists()
        rule.onNodeWithText("~40 L of water").assertExists()
        rule.onNodeWithText("Forest").assertExists()
        rule.onNodeWithText("~270 tree-days").assertExists()
        rule.onNodeWithText("~16 kg CO\u2082").assertExists()
        rule.onNodeWithText("How this is estimated", substring = true).assertExists()
        rule.onNodeWithText("never measured", substring = true).assertExists()
        rule.onNodeWithText("Epoch AI, 2025", substring = true).assertExists()
    }

    @Test
    fun projectsAndNodesSitSideBySideOnAWideScreenAndStackOnANarrowOne() {
        val heading = SemanticsMatcher.keyIsDefined(SemanticsProperties.Heading)
        fun top(title: String) = rule.onNode(hasText(title) and heading).fetchSemanticsNode().positionInRoot.y

        showSidebar(Reported, 1400.dp)
        assertEquals(top("Projects"), top("Nodes"))
    }

    @Test
    fun projectsAndNodesStackOnANarrowScreen() {
        val heading = SemanticsMatcher.keyIsDefined(SemanticsProperties.Heading)

        showSidebar(Reported, 700.dp)

        val projects = rule.onNode(hasText("Projects") and heading).fetchSemanticsNode().positionInRoot.y
        val nodes = rule.onNode(hasText("Nodes") and heading).fetchSemanticsNode().positionInRoot.y
        assertEquals(true, nodes > projects)
    }

    @Test
    fun anAgentThatReportsNoUsageIsNamedAndShowsTurnsAndTimeOnly() {
        showSidebar(Reported)

        rule.onNodeWithText("Codex did not report token usage in this range, so only its turns and time working are counted.").assertExists()
        assertEquals(true, shown("Not reported"))
    }

    @Test
    fun choosingARangeAndANodeAsksAgainWithThem() {
        showSidebar(Reported)

        rule.onNodeWithText("30 days").performClick()
        rule.waitForIdle()
        assertEquals("range=30d", asked.last())

        rule.onNodeWithContentDescription("Choose nodes").performClick()
        rule.onNodeWithText("atlas").performClick()
        rule.waitForIdle()
        assertEquals("range=30d&node_id=n2", asked.last())
    }

    @Test
    fun allTimeAsksForTheWholeHistoryAndChartsTokensPerDay() {
        showSidebar(Reported.replace("\"range\":\"7d\"", "\"range\":\"all\""))

        rule.onNodeWithText("All time").performClick()
        rule.waitForIdle()

        assertEquals("range=all", asked.last())
        rule.onNodeWithText("Tokens per day, all time").assertExists()
        rule.onNodeWithText("Peak 181M on 2 Oct, 182M in total.").assertExists()
    }

    @Test
    fun aRangeWithNoTurnsSaysSoInsteadOfShowingZeros() {
        showSidebar(Empty)

        rule.onNodeWithText("No turns in this range yet.").assertExists()
    }
}
