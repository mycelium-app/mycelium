package mycelium.desktop

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
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

private class FilterPlatform(private val filter: String) : Platform {
    override val originUrl: String? = null
    override val signsInThroughTheBrowser = false
    override fun loadConnection(): mycelium.Connection? = null
    override fun saveConnection(connection: mycelium.Connection?) = Unit
    override fun device() = mycelium.DeviceInfo("Desktop", "host", "linux", "x86_64")
    override fun approveCode(): String? = null
    override fun currentPath() = "/"
    override fun navigate(url: String) = Unit
    override fun openUrl(url: String) = Unit
    override fun copyToClipboard(text: String) = Unit
    override fun loadPreference(key: String): String? = filter.takeIf { key == "conversation-group-filter" }
    override fun savePreference(key: String, value: String) = Unit
}

private fun scopedConversation(id: String, title: String, group: String?, archived: Boolean = false) =
    """{"id":"$id","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/$id","title":"$title","status":"done","acp_session_id":"s","mode":null,"model":null,"group_id":${group?.let { "\"$it\"" }},
"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z","archived_at":${if (archived) "\"2026-10-23T10:00:00Z\"" else "null"},"pinned_at":null}"""

private fun scopedProject(id: String, name: String, group: String) =
    """{"id":"$id","node_id":"n1","name":"$name","path":"/work/$name","group_id":"$group","created_at":"2026-10-22T10:00:00Z"}"""

private fun scopedTask(id: String, title: String, project: String) =
    """{"id":"$id","node_id":"n1","agent":"claude","cwd":"/work/app","project_id":"$project","branch":null,"title":"$title","status":"open","created_at":"2026-10-22T10:00:00Z"}"""

private fun scopedWorkflow(id: String, title: String, conversation: String) =
    """{"id":"$id","parent_id":null,"conversation_id":"$conversation","title":"$title","status":"running","round":1,"plan":{"steps":[]},"steps":[]}"""

private fun scopedSchedule(id: String, prompt: String, conversation: String?, project: String? = null) =
    """{"id":"$id","node_id":"n1","agent":"claude","cwd":"/work/app","project_id":${project?.let { "\"$it\"" }},"conversation_id":${conversation?.let { "\"$it\"" }},"prompt":"$prompt","cron":"0 6 * * 1","each_run":"new","paused":false,
"next_run_at":"2099-01-01T06:00:00Z","last_run_at":null,"last_conversation_id":null,"created_at":"2026-10-22T10:00:00Z"}"""

private fun scopedArtifact(id: String, title: String, project: String?) =
    """{"id":"$id","owner":"u","conversation_id":null,"project_ids":${project?.let { "[\"$it\"]" } ?: "[]"},"kind":"page","title":"$title","slug":"$id-ab12","public":false,"latest_version":1,"thumbnail_version":null,"url":null,
"created_at":"2026-10-22T09:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val ScopedNodes = """[{"id":"n1","name":"laptop","hostname":"laptop","os":"linux","arch":"x86_64","version":"0.1.0","online":true,"draining":false,"agents":[],"logins":[]}]"""

private const val ScopedGroups = """[{"id":"g1","name":"slop","folders":[],"created_at":"2026-10-22T10:00:00Z"},{"id":"g2","name":"work","folders":[],"created_at":"2026-10-22T10:00:00Z"}]"""

class GroupScopeTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun show(route: Route, filter: String = "g1") {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/experiments") -> EnabledExperiments
                path.endsWith("/conversations") && request.url.parameters["archived"] == "true" -> "[${scopedConversation("a1", "Slop archived", "g1", true)},${scopedConversation("a2", "Work archived", "g2", true)}]"
                path.endsWith("/conversations") -> "[${scopedConversation("c1", "Slop chat", "g1")},${scopedConversation("c2", "Work chat", "g2")}]"
                path.endsWith("/groups") -> ScopedGroups
                path.endsWith("/nodes") -> ScopedNodes
                path.endsWith("/projects") -> "[${scopedProject("p1", "slop-app", "g1")},${scopedProject("p2", "work-app", "g2")}]"
                path.endsWith("/worktrees") -> "[]"
                path.endsWith("/tasks") -> "[${scopedTask("t1", "Slop task", "p1")},${scopedTask("t2", "Work task", "p2")}]"
                path.endsWith("/workflows") -> "[${scopedWorkflow("w1", "Slop workflow", "c1")},${scopedWorkflow("w2", "Work workflow", "c2")}]"
                path.endsWith("/schedules") -> "[${scopedSchedule("s1", "Slop schedule", "c1")},${scopedSchedule("s2", "Work schedule", null, "p2")},${scopedSchedule("s3", "Loose schedule", null)}]"
                path.endsWith("/schedule-runs") -> "[]"
                path.endsWith("/artifacts") -> "[${scopedArtifact("f1", "Slop page", "p1")},${scopedArtifact("f2", "Work page", "p2")},${scopedArtifact("f3", "Loose page", null)}]"
                path.endsWith("/events") || path.endsWith("/queue") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        runBlocking { store.refresh() }
        rule.setContent { MyceliumTheme(true) { Shell(store, FilterPlatform(filter), Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = route) } }
    }

    private fun showSchedules(filter: String = "g1") {
        show(Route.Settings, filter)
        rule.onNodeWithText("Schedules").performClick()
    }

    private fun shown(title: String): Int {
        rule.waitForIdle()

        return rule.onAllNodesWithText(title, substring = true).fetchSemanticsNodes().size
    }

    private fun waitFor(title: String) {
        rule.waitUntil(5_000) { rule.onAllNodesWithText(title, substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun historyShowsOnlyTheSelectedGroupsConversations() {
        show(Route.Archive)

        waitFor("Slop archived")

        assertEquals(0, shown("Work archived"))
    }

    @Test
    fun artefactsShowOnlyTheSelectedGroupsProjects() {
        show(Route.Artifacts)

        waitFor("Slop page")

        assertEquals(0, shown("Work page"))
        assertEquals(0, shown("Loose page"))
    }

    @Test
    fun tasksShowOnlyThoseFiledAgainstTheSelectedGroupsProjects() {
        show(Route.Tasks)

        waitFor("Slop task")

        assertEquals(0, shown("Work task"))
    }

    @Test
    fun workflowsShowOnlyThoseStartedFromTheSelectedGroupsConversations() {
        show(Route.Workflows)

        waitFor("Slop workflow")

        assertEquals(0, shown("Work workflow"))
    }

    @Test
    fun schedulesShowOnlyThoseRunningInTheSelectedGroup() {
        showSchedules()

        waitFor("Slop schedule")

        assertEquals(0, shown("Work schedule"))
        assertEquals(0, shown("Loose schedule"))
    }

    @Test
    fun noGroupShowsOnlySchedulesWithoutOne() {
        showSchedules("none")

        waitFor("Loose schedule")

        assertEquals(0, shown("Slop schedule"))
        assertEquals(0, shown("Work schedule"))
    }

    @Test
    fun allGroupsShowsEverySchedule() {
        showSchedules("all")

        waitFor("Slop schedule")

        assertEquals(1, shown("Work schedule"))
        assertEquals(1, shown("Loose schedule"))
    }
}
