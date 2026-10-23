package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.unit.dp
import kotlin.time.Clock
import kotlin.time.Duration.Companion.days
import kotlin.time.Duration.Companion.hours
import kotlin.time.Duration.Companion.minutes
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.state.Lane
import mycelium.state.Store
import mycelium.state.boardCards
import mycelium.ui.MyTheme
import mycelium.ui.MoveSheet
import mycelium.ui.NewChatScreen
import mycelium.ui.ActivityScreen
import mycelium.ui.AccountsSettings
import mycelium.ui.ForgesSettings
import mycelium.ui.InstructionsSettings
import mycelium.ui.Route
import mycelium.ui.SessionsSettings
import mycelium.ui.Shell
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val BackupsId = "77777777-7777-7777-7777-777777777777"
private const val SideChatId = "88888888-8888-8888-8888-888888888888"
private const val WillowId = "bbbbbbbb-0000-0000-0000-000000000001"
private const val AtlasNodeId = "bbbbbbbb-0000-0000-0000-000000000002"
private const val BuildId = "bbbbbbbb-0000-0000-0000-000000000003"
private const val GithubId = "cccccccc-0000-0000-0000-000000000001"
private const val GitlabId = "cccccccc-0000-0000-0000-000000000002"

private fun ago(minutes: Long = 0, hours: Long = 0, days: Long = 0) = (Clock.System.now() - minutes.minutes - hours.hours - days.days).toString()

private fun ahead(hours: Long = 0, days: Long = 0) = (Clock.System.now() + hours.hours + days.days).toString()

private fun conversationJson(id: String, title: String, status: String, node: String, updated: String, agent: String = "claude", parent: String? = null, side: String? = null, cwd: String = "/home/sam/projects/acme-backups", account: String? = null, project: String? = null) =
    """{"id":"$id","owner":"u","node_id":"$node","agent":"$agent","cwd":"$cwd","title":"$title","status":"$status","mode":"acceptEdits","model":"Opus 5.5",${if (project != null) """"project_id":"$project","acp_session_id":"s",""" else ""}${if (account != null) """"account":"$account",""" else ""}
    "created_at":"${ago(days = 2)}","updated_at":"$updated","archived_at":null${if (parent != null) ""","parent_id":"$parent","side_kind":"$side"""" else ""}}"""

private fun nodeJson(id: String, name: String, os: String, icon: String, online: Boolean = true, revision: String = "afad776", older: Int = 0, address: String, version: String = "0.1.10", extraLogins: String = "") =
    """{"id":"$id","name":"$name","icon":"$icon","hostname":"$name","os":"$os","arch":"x86_64","version":"$version","address":"$address","online":$online,"draining":false,
    "last_seen_at":"${ago(minutes = 1)}","instructions_revision":"$revision","sessions_on_older_instructions":$older,
    "agents":[{"agent":"claude","version":"2.1.308","available":true},{"agent":"codex","version":"0.163.0","available":true}],
    "logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in","usage_left":18},{"agent":"claude","account":"work@acme.example","method":"token","state":"signed_in","usage_left":77}$extraLogins]}"""

private val BoardNodes = listOf(
    nodeJson(WillowId, "willow", "linux", "laptop", older = 2, address = "100.64.4.18"),
    nodeJson(AtlasNodeId, "atlas", "linux", "server", online = false, revision = "85d49d3", address = "10.0.0.12", extraLogins = """,{"agent":"claude","account":"side@example.com","method":"token","state":"signed_in"}"""),
    nodeJson(BuildId, "build-01", "linux", "server", address = "10.0.0.20"),
).joinToString(",", "[", "]")

private val BoardConversations = listOf(
    conversationJson(BackupsId, "Backups", "review", WillowId, ago(minutes = 2), account = "work@acme.example"),
    conversationJson("11111111-1111-1111-1111-111111111111", "Bump @immich/ui in midden", "working", AtlasNodeId, ago(hours = 7), cwd = "/home/sam/projects/midden"),
    conversationJson("55555555-5555-5555-5555-555555555555", "Uploads survive a 503", "working", AtlasNodeId, ago(hours = 8), agent = "codex", cwd = "/home/sam/projects/midden"),
    conversationJson("66666666-6666-6666-6666-666666666666", "Flux image automation for vm", "done", AtlasNodeId, ago(days = 1), cwd = "/home/sam/projects/vm"),
).joinToString(",", "[", "]")

private fun event(conversation: String, seq: Int, kind: String, payload: String) = """{"conversation_id":"$conversation","seq":$seq,"kind":"$kind","payload":$payload}"""

private fun userMessage(conversation: String, seq: Int, text: String) = event(conversation, seq, "user_message", """{"text":"$text"}""")

private fun agentText(conversation: String, seq: Int, text: String) =
    event(conversation, seq, "acp_update", """{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"$text"}}}""")

private fun turnEnd(conversation: String, seq: Int) = event(conversation, seq, "turn_end", """{"stopReason":"end_turn"}""")

private fun tool(conversation: String, seq: Int, title: String, status: String) =
    event(conversation, seq, "acp_update", """{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t$seq","title":"$title","kind":"execute","status":"$status","rawInput":{"command":"$title"}}}""")

private fun jsonText(text: String) = text.replace("\\", "\\\\").replace("\"", "\\\"").replace("\n", "\\n")

private fun list(vararg items: String) = items.joinToString(",", "[", "]")

private val BoardProjects = list(
    """{"id":"p1","node_id":"$WillowId","name":"acme-backups","path":"/home/sam/projects/acme-backups","forge_id":"$GithubId","repository":"acme-org/acme-backups","created_at":"${ago(days = 30)}","group_id":"g1"}""",
    """{"id":"p2","node_id":"$WillowId","name":"mycelium","path":"/home/sam/projects/mycelium","forge_id":"$GitlabId","repository":"slop/mycelium","created_at":"${ago(days = 30)}","group_id":"g2"}""",
    """{"id":"p3","node_id":"$AtlasNodeId","name":"mycelium","path":"/srv/mycelium","forge_id":"$GitlabId","repository":"slop/mycelium","created_at":"${ago(days = 30)}"}""",
    """{"id":"p4","node_id":"$AtlasNodeId","name":"homelab","path":"/srv/homelab","forge_id":"$GitlabId","repository":"homelab/homelab","created_at":"${ago(days = 30)}","group_id":"g3"}""",
)

private val BoardGroups = list(
    """{"id":"g1","name":"work","created_at":"${ago(days = 30)}"}""",
    """{"id":"g2","name":"slop","created_at":"${ago(days = 30)}"}""",
    """{"id":"g3","name":"homelab","created_at":"${ago(days = 30)}"}""",
)

private val BoardForges = list(
    """{"id":"$GithubId","kind":"github","base_url":"https://github.com","account":"samexample","token_hint":"…a1b2","organisations":["acme-org","immich-app","samexample"],"created_at":"${ago(days = 30)}"}""",
    """{"id":"$GitlabId","kind":"gitlab","base_url":"https://gitlab.example.com","account":"sam","token_hint":"…c3d4","organisations":["slop","homelab"],"created_at":"${ago(days = 30)}"}""",
)

private val BoardRepositories = """{"repositories":[
{"forge_id":"$GithubId","owner":"acme-org","name":"acme-backups","full_name":"acme-org/acme-backups"},
{"forge_id":"$GitlabId","owner":"slop","name":"mycelium","full_name":"slop/mycelium"},
{"forge_id":"$GithubId","owner":"immich-app","name":"immich","full_name":"immich-app/immich"},
{"forge_id":"$GitlabId","owner":"homelab","name":"homelab","full_name":"homelab/homelab"}],
"organisations":[{"forge_id":"$GithubId","name":"acme-org"},{"forge_id":"$GitlabId","name":"slop"}]}"""

private val BoardTasks = list(
    """{"id":"k0","node_id":"$WillowId","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Tag v0.1.11 now, or wait for the Firefox fix?","status":"question","options":["Tag now","Wait"],"source_title":"Release","created_at":"${ago(hours = 6)}"}""",
    """{"id":"kp","node_id":"$WillowId","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Cache cargo and Gradle in the release jobs","status":"proposed","source_title":"Release","created_at":"${ago(hours = 6)}"}""",
    """{"id":"k1","node_id":"$WillowId","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Rebuild install.md for the new flags","status":"open","created_at":"${ago(hours = 5)}"}""",
    """{"id":"k2","node_id":"$WillowId","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Verify v0.1.11 from S3","status":"open","depends_on":["k1"],"created_at":"${ago(hours = 4)}"}""",
    """{"id":"k3","node_id":"$AtlasNodeId","agent":"codex","cwd":"/srv","title":"Rotate the S3 keys","status":"open","created_at":"${ago(hours = 3)}"}""",
    """{"id":"kc","node_id":"$WillowId","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Merge the three Gradle jobs","status":"claimed","conversation_id":"11111111-1111-1111-1111-111111111111","created_at":"${ago(hours = 2)}"}""",
    *(1..13).map { """{"id":"kd$it","node_id":"$WillowId","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Finished task $it","status":"done","created_at":"${ago(days = 2)}"}""" }.toTypedArray(),
)

private val BoardSchedules = list(
    """{"id":"s1","node_id":"$WillowId","agent":"claude","cwd":"/home/sam/projects/midden","project_id":"p1","prompt":"Renovate sweep: update and test dependencies","cron":"0 6 * * 1","each_run":"new","paused":false,"next_run_at":"${ahead(days = 3)}","last_run_at":"${ago(days = 4)}","created_at":"${ago(days = 30)}"}""",
    """{"id":"s2","node_id":"$WillowId","agent":"claude","cwd":"/home/sam/projects/acme-backups","project_id":"p1","prompt":"Check backups restored cleanly overnight","cron":"30 7 * * *","each_run":"same","paused":false,"next_run_at":"${ahead(hours = 11)}","last_run_at":"${ago(hours = 2)}","created_at":"${ago(days = 30)}"}""",
    """{"id":"s3","node_id":"$WillowId","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","prompt":"Summarise what shipped this week","cron":"0 17 * * 5","each_run":"new","paused":true,"next_run_at":"${ahead(days = 4)}","created_at":"${ago(days = 30)}"}""",
)

private val BoardScheduleRuns = list(
    """{"id":"r1","schedule_id":"s1","ran_at":"${ago(days = 4)}","outcome":"started","conversation_id":"$BackupsId"}""",
    """{"id":"r2","schedule_id":"s2","ran_at":"${ago(hours = 2)}","outcome":"started","conversation_id":"66666666-6666-6666-6666-666666666666"}""",
)

private val BoardSessions = list(
    """{"id":"x1","kind":"browser","label":"Firefox on Linux","address":"100.64.4.18","last_used_at":"${ago()}","current":true}""",
    """{"id":"x2","kind":"client","label":"Desktop app on willow","hostname":"willow","address":"100.64.4.18","last_used_at":"${ago(minutes = 12)}"}""",
    """{"id":"x3","kind":"client","label":"Android app on Pixel 9","address":"100.64.7.3","last_used_at":"${ago(hours = 2)}"}""",
    """{"id":"x4","kind":"browser","label":"Chrome on Android","address":"100.64.7.3","last_used_at":"${ago(days = 3)}"}""",
)

private val BoardInstructions = """{"source":{"forge_id":"$GithubId","repository":"samexample/agents-md","path":"AGENTS.md"},"revision":"afad776","layers":[
{"layer":"global","name":"Global","path":"AGENTS.md","text":"## Diffs\n\n- Change only what the task requires.\n- When I ask to remove or change one element, leave every other element where it is and as it is. Do not move, restyle or regroup neighbouring controls to fill the gap.","version":{"id":"afad776","created_at":"${ago(hours = 2)}"}},
{"layer":"group:slop","name":"slop","text":""},{"layer":"group:work","name":"work","text":""},{"layer":"group:homelab","name":"homelab","text":""}]}"""

private val BoardActivity = """{"range":"7d","totals":{"key":"all","name":"All","turns":4212,"reported_turns":4212,"working_ms":900000000,"input_tokens":60000000,"output_tokens":22000000,"cache_read_tokens":90000000,"cache_write_tokens":10000000,"cost_usd":1940.0},
"hours":[${(16..22).flatMap { day -> (0..23).map { day to it } }.joinToString(",") { (day, hour) -> """{"key":"2026-10-${day}T${hour.toString().padStart(2, '0')}","name":"hour","turns":${(day + hour) % 5 + 1},"reported_turns":1,"input_tokens":${(if (hour in 8..20) 900_000 else 150_000) + (day * 37 + hour * 91) % 700_000},"output_tokens":100000}""" }}],
"days":[${(1..7).joinToString(",") { """{"key":"2026-10-${15 + it}","name":"Day $it","turns":${it * 50},"reported_turns":${it * 50},"working_ms":${it * 3_600_000},"input_tokens":${it * 3_000_000},"output_tokens":${it * 1_000_000},"cache_read_tokens":${it * 6_000_000}}""" }}],
"projects":[{"key":"p2","name":"mycelium","turns":900,"reported_turns":900,"input_tokens":30000000,"output_tokens":11000000,"cache_read_tokens":30000000,"cost_usd":760.0},{"key":"p1","name":"acme-backups","turns":700,"reported_turns":700,"input_tokens":20000000,"output_tokens":8000000,"cache_read_tokens":20000000,"cost_usd":510.0},{"key":"p5","name":"midden","turns":400,"reported_turns":400,"input_tokens":10000000,"output_tokens":3000000,"cache_read_tokens":18000000,"cost_usd":330.0}],
"nodes":[{"key":"$WillowId","name":"willow","turns":2800,"reported_turns":2800,"input_tokens":50000000,"output_tokens":21000000,"cache_read_tokens":50000000},{"key":"$AtlasNodeId","name":"atlas","turns":1400,"reported_turns":1400,"input_tokens":10000000,"output_tokens":1000000,"cache_read_tokens":50000000}]}"""

private val BoardResources = """{"latest":{"at_ms":${Clock.System.now().toEpochMilliseconds()},"available":true,"cpu_percent":38.0,"cores":8,"load":[2.1,1.8,1.5],
"memory_total_bytes":34359738368,"memory_available_bytes":11400000000,"swap_total_bytes":0,"swap_free_bytes":0,
"disks":[{"mount":"/","total_bytes":512000000000,"free_bytes":13000000000}],
"processes":[{"pid":10,"name":"gradle daemon","cpu_percent":212.0,"memory_bytes":3300000000,"agent":false,"stoppable":true},{"pid":11,"name":"claude","cpu_percent":14.0,"memory_bytes":670000000,"agent":true,"stoppable":true,"conversation_id":"$BackupsId"},{"pid":12,"name":"postgres","cpu_percent":3.0,"memory_bytes":440000000}],
"ports":[{"port":5173,"address":"127.0.0.1","pid":13,"process":"vite"}],
"docker":{"running":9,"reclaimable_bytes":21800000000,"containers":[${(1..14).joinToString(",") { """{"name":"service-$it","state":"${if (it <= 9) "running" else "exited"}"}""" }}]}},
"history":[${(0..59).joinToString(",") { """{"at_ms":${Clock.System.now().toEpochMilliseconds() - (60 - it) * 10_000},"cpu_percent":${25 + (it * 7) % 30}.0}""" }}]}"""

private val BoardIssues = """{"issues":[
{"forge_id":"$GithubId","repository":"acme-org/acme-backups","number":412,"title":"Restore fails silently after a 503","url":"https://github.com/acme-org/acme-backups/issues/412","pull_requests":[]},
{"forge_id":"$GithubId","repository":"acme-org/acme-backups","number":398,"title":"Move restore copy into shared strings","url":"https://github.com/acme-org/acme-backups/issues/398","pull_requests":[{"number":77,"label":"MR !77","url":"https://git.example/acme-org/acme-backups/-/merge_requests/77","state":"open"}]},
{"forge_id":"$GithubId","repository":"acme-org/acme-backups","number":377,"title":"Android restore progress jumps backwards","url":"https://github.com/acme-org/acme-backups/issues/377","pull_requests":[]}]}"""

private val BoardWorktrees = list(
    """{"id":"w1","project_id":"p1","branch":"issue/398-restore-strings","path":"/home/sam/projects/acme-backups-398","issue_number":398,"created_at":"${ago(days = 1)}"}""",
)

private val BoardProviders = list(
    """{"id":"mp1","kind":"anthropic","name":"Anthropic","key_hint":"…f00d","models":["Opus 5.5","Sonnet 5.5"],"default_model":"Opus 5.5","created_at":"${ago(days = 30)}"}""",
    """{"id":"mp2","kind":"openai","name":"Ollama","base_url":"http://localhost:11434/v1","models":["llama4"],"default_model":"llama4","created_at":"${ago(days = 30)}"}""",
)

private fun boardApi(extra: Map<String, String> = emptyMap()) = fakeApi(
    BoardNodes,
    linkedMapOf(
        "/conversations/$BackupsId/events" to list(
            userMessage(BackupsId, 1, "Bump the restore tool's dependencies."),
            agentText(BackupsId, 2, "Updated 14 packages; tests pass."),
        ),
        "/conversations" to BoardConversations,
        "/projects" to BoardProjects,
        "/groups" to BoardGroups,
        "/forges" to BoardForges,
        "/repositories" to BoardRepositories,
        "/tasks" to BoardTasks,
        "/schedules" to BoardSchedules,
        "/schedule-runs" to BoardScheduleRuns,
        "/sessions" to BoardSessions,
        "/instructions" to BoardInstructions,
        "/worktrees" to BoardWorktrees,
        "/issues" to BoardIssues,
        "/model-providers" to BoardProviders,
    ) + extra,
)

@OptIn(InternalComposeUiApi::class)
private fun click(scene: ImageComposeScene, x: Float, y: Float) {
    scene.render(100_000_000L)
    scene.sendPointerEvent(PointerEventType.Move, Offset(x, y))
    scene.sendPointerEvent(PointerEventType.Press, Offset(x, y))
    scene.render(116_000_000L)
    scene.sendPointerEvent(PointerEventType.Release, Offset(x, y))
    scene.render(132_000_000L)
}

@OptIn(InternalComposeUiApi::class)
class BoardsRenderTest {
    private val me = Me("u", "sam@example.com", "Sam")

    private fun store(extra: Map<String, String> = emptyMap(), open: String? = BackupsId, board: Boolean = false): Store {
        val store = Store(boardApi(extra), CoroutineScope(Dispatchers.Unconfined))

        runBlocking {
            store.refresh()
            store.refreshProjects()

            if (board) {
                store.refreshBoard()
                store.refreshSchedules()
            }

            if (open != null) {
                store.open(open)
            }
        }

        return store
    }

    private fun shell(route: Route, extra: Map<String, String> = emptyMap(), open: String? = BackupsId, board: Boolean = false): @androidx.compose.runtime.Composable () -> Unit {
        val store = store(extra, open, board)

        return { Shell(store, PlatformStub, me, onSignOut = {}, initialRoute = route) }
    }

    private fun section(name: String, content: @androidx.compose.runtime.Composable (Store) -> Unit) {
        val store = store()

        assertTrue(renderAt(name, true, false) { Column(Modifier.fillMaxSize().background(MyTheme.colors.bgContent).padding(24.dp)) { content(store) } }.length() > 1_000)
    }

    @Test
    fun tasksAndSchedules() {
        val idle = mapOf("/conversations" to BoardConversations.dropLast(1) + "," + conversationJson("88888888-8888-8888-8888-888888888888", "Release", "review", WillowId, ago(minutes = 9), cwd = "/home/sam/projects/mycelium", project = "p2") + "]")

        assertTrue(renderAt("board-tasks", true, false, content = shell(Route.Tasks, idle, board = true)).length() > 1_000)

        val details = boardCards(store(idle, board = true).state.value).getValue(Lane.Open).associate { it.title to it.details }
        assertEquals("mycelium · any idle conversation", details.getValue("Rebuild install.md for the new flags"))
        assertTrue(!details.getValue("Rotate the S3 keys").contains("idle"))
        assertTrue(renderAt("board-schedules", true, false, content = shell(Route.Schedules, board = true)).length() > 1_000)
    }

    @Test
    fun settingsSections() {
        section("board-settings-accounts") { AccountsSettings(it) }
        section("board-settings-sessions") { SessionsSettings(it, onSignOut = {}) }
        section("board-instructions") { InstructionsSettings(it) }
        section("board-forges") { ForgesSettings(it) }
    }

    @Test
    fun activityNodeProjectsIssues() {
        val activity = mapOf("/activity" to BoardActivity)
        assertTrue(renderAt("board-activity-view", true, false, content = shell(Route.Activity, activity)).length() > 1_000)
        assertTrue(renderAt("board-node-resources", true, false, interact = { click(it, 393f, 56f) }, content = shell(Route.Node(WillowId), mapOf("/watch" to BoardResources))).length() > 1_000)
        assertTrue(renderAt("board-projects", true, false, content = shell(Route.Projects, board = true)).length() > 1_000)
        assertTrue(renderAt("board-issues", true, false, content = shell(Route.Issues, board = true)).length() > 1_000)
    }

    @Test
    fun conversationBoards() {
        val working = mapOf("/conversations" to BoardConversations.replace("\"id\":\"$BackupsId\",\"owner\":\"u\",\"node_id\":\"$WillowId\",\"agent\":\"claude\",\"cwd\":\"/home/sam/projects/acme-backups\",\"title\":\"Backups\",\"status\":\"review\"", "\"id\":\"$BackupsId\",\"owner\":\"u\",\"node_id\":\"$WillowId\",\"agent\":\"claude\",\"cwd\":\"/home/sam/projects/acme-backups\",\"title\":\"Backups\",\"status\":\"working\""))
        val goal = working + mapOf(
            "/conversations/$BackupsId/events" to list(
                agentText(BackupsId, 1, "Turn 7: the Android build now passes; two e2e tests still fail on the restore screen."),
                tool(BackupsId, 2, "pnpm e2e restore", "in_progress"),
            ),
            "/goal" to """{"conversation_id":"$BackupsId","text":"every test passes on main, including e2e","token_budget":5000000,"tokens_used":2100000,"turns":7,"state":"active"}""",
        )
        assertTrue(renderAt("board-goals", true, false, content = shell(Route.Chat(BackupsId), goal)).length() > 1_000)

        val queued = working + mapOf(
            "/conversations/$BackupsId/events" to list(
                userMessage(BackupsId, 1, "Move the restore copy into the shared strings file."),
                agentText(BackupsId, 2, "Moving the six restore strings into `strings.ts`."),
                tool(BackupsId, 3, "pnpm -r test", "in_progress"),
            ),
            "/queue" to list(
                """{"id":"q1","conversation_id":"$BackupsId","position":1,"text":"Then open a merge request against main","created_at":"${ago(minutes = 3)}"}""",
                """{"id":"q2","conversation_id":"$BackupsId","position":2,"text":"Check the Android strings too","created_at":"${ago(minutes = 2)}"}""",
            ),
        )
        assertTrue(renderAt("board-queued", true, false, content = shell(Route.Chat(BackupsId), queued)).length() > 1_000)

        val titled = mapOf(
            "/conversations" to BoardConversations.replace("\"title\":\"Backups\"", "\"title\":\"Move restore copy into shared strings\""),
            "/conversations/$BackupsId/events" to list(
                userMessage(BackupsId, 1, "Move the restore copy into the shared strings file."),
                agentText(BackupsId, 2, "Moved the six restore strings into `strings.ts` and updated the three screens that used them. Tests pass. What next?\\n\\n1. Open a merge request\\n2. Do the Android strings too\\n3. Run the e2e suite"),
                turnEnd(BackupsId, 3),
            ),
        )
        assertTrue(renderAt("board-titles", true, false, content = shell(Route.Chat(BackupsId), titled)).length() > 1_000)

        val diff = "@@ -1,5 +1,5 @@\n-  const title = \"Restore your backup\"\n+  const title = strings.restore.title\n   const retry = useRetry(RESTORE_BACKOFF_MS)\n-  const failed = \"Could not restore. Try again.\"\n+  const failed = strings.restore.failed"
        val walkthrough = mapOf(
            "/walkthrough" to """{"id":"wt","conversation_id":"$BackupsId","created_at":"${ago(minutes = 5)}","steps":[
{"title":"Add the shared strings file","section":"Strings","icon":"data"},
{"title":"Restore reads its copy from strings","key":true,"section":"Strings","icon":"data","explanation":"The restore screen no longer holds its own copy, so a change to the wording reaches every screen at once.","files":[{"path":"src/restore/Screen.tsx","diff":"${jsonText(diff)}"}]},
{"title":"Update the retry screen","section":"Retry logic","icon":"logic"},{"title":"Cover the restore flow","section":"Tests","icon":"tests"}]}""",
            "/reads" to "[0]",
        )
        assertTrue(renderAt("board-walkthrough", true, false, interact = { click(it, 437f, 56f); click(it, 330f, 190f) }, content = shell(Route.Chat(BackupsId), walkthrough)).length() > 1_000)
    }

    @Test
    fun movesChatsNewProject() {
        val plan = mapOf("/move-plan" to """{"movable":true,"branch":"issue/398-restore-strings","changed_files":3,"agent_session":true,"stays_ports":[5173],"stays_processes":["dev server"]}""")
        val moveStore = store(plan)
        assertTrue(renderAt("board-moves", true, false) {
            Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) {
                MoveSheet(moveStore, moveStore.state.value, moveStore.state.value.conversations.getValue(BackupsId), {}, {})
            }
        }.length() > 1_000)
        assertTrue(renderAt("board-new-project", true, false, interact = { click(it, 1377f, 20f); click(it, 548f, 368f) }, content = shell(Route.Projects, board = true)).length() > 1_000)
        val chats = store(emptyMap(), null)
        assertTrue(renderAt("board-chats-new", true, false) {
            Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) { NewChatScreen(chats, onCreated = {}) }
        }.length() > 1_000)
    }

    @Test
    fun accountsSideChatsAndChats() {
        assertTrue(renderAt("board-accounts", true, false, interact = { click(it, 937f, 878f) }, content = shell(Route.Chat(BackupsId))).length() > 1_000)

        val side = mapOf(
            "/conversations" to BoardConversations.replaceFirst("]", "," + conversationJson(SideChatId, "btw: where else is the b...", "done", WillowId, ago(minutes = 1), parent = BackupsId, side = "btw") + "]"),
            "/conversations/$BackupsId/events" to list(
                agentText(BackupsId, 1, "The restore screen reads the retry delay from `RESTORE_BACKOFF_MS`, which defaults to 2000."),
                turnEnd(BackupsId, 2),
            ),
        )
        assertTrue(renderAt("board-sidechat", true, false, content = shell(Route.Chat(BackupsId), side)).length() > 1_000)

        val ui = """{"type":"column","children":[{"type":"heading","text":"Restore time for 2 TB"},{"type":"bar_chart","bars":[{"label":"Full","value":4.1,"display":"4.1 h"},{"label":"Differential","value":6.3,"display":"6.3 h"},{"label":"Incremental","value":9.2,"display":"9.2 h"}]},{"type":"row","children":[{"type":"button","label":"Use differential","action":{"type":"send_message","text":"Use differential"}},{"type":"button","label":"Explain the trade-off","action":{"type":"side_chat","text":"Explain the trade-off"}}]}]}"""
        val chatId = "99999999-9999-9999-9999-999999999999"
        fun chat(id: String, title: String, updated: String) = conversationJson(id, title, "done", "", updated, agent = "chat").replace("\"mode\"", "\"provider_id\":\"mp1\",\"mode\"")
        val chats = mapOf(
            "/conversations" to list(chat(chatId, "Compare restore strategies", ago(minutes = 1)), chat("99999999-9999-9999-9999-99999999999a", "Trip to Lisbon", ago(hours = 2)), chat("99999999-9999-9999-9999-99999999999b", "Explain NetBird DNS", ago(days = 3))),
            "/conversations/$chatId/events" to list(
                userMessage(chatId, 1, "Compare full, incremental and differential restores for 2 TB."),
                agentText(chatId, 2, jsonText("```mycelium-ui\n$ui\n```")),
                turnEnd(chatId, 3),
            ),
        )
        assertTrue(renderAt("board-chats", true, false, content = shell(Route.Chat(chatId), chats, open = chatId)).length() > 1_000)
    }
}
