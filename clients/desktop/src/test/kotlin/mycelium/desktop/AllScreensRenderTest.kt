package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.unit.Density
import kotlin.time.Clock
import kotlin.time.Duration.Companion.days
import kotlin.time.Duration.Companion.hours
import kotlin.time.Duration.Companion.minutes
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.builtins.ListSerializer
import mycelium.api.ChatToolServer
import mycelium.api.Me
import mycelium.api.MyceliumJson
import mycelium.state.Store
import mycelium.ui.Accent
import mycelium.ui.Appearance
import mycelium.ui.ChatToolsSettings
import mycelium.ui.DeleteConversationDialog
import mycelium.ui.IntegrationUsage
import mycelium.ui.IntegrationsActivity
import mycelium.ui.LocalAppearance
import mycelium.ui.LocalPlatform
import mycelium.ui.LocalTouch
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.SettingsScreen
import mycelium.ui.SettingsSection
import mycelium.ui.Shell
import mycelium.ui.integrationsOf
import org.jetbrains.skia.EncodedImageFormat
import org.junit.Rule
import java.io.File
import kotlin.test.Test
import kotlin.test.assertTrue

private const val Main = "11111111-1111-1111-1111-111111111111"
private const val Codex = "55555555-5555-5555-5555-555555555555"
private const val ChatA = "99999999-9999-9999-9999-999999999999"
private const val Willow = "bbbbbbbb-0000-0000-0000-000000000001"
private const val Atlas = "bbbbbbbb-0000-0000-0000-000000000002"
private const val Github = "cccccccc-0000-0000-0000-000000000001"
private const val Gitlab = "cccccccc-0000-0000-0000-000000000002"

private fun ago(minutes: Long = 0, hours: Long = 0, days: Long = 0) = (Clock.System.now() - minutes.minutes - hours.hours - days.days).toString()

private fun ahead(hours: Long = 0, days: Long = 0) = (Clock.System.now() + hours.hours + days.days).toString()

private fun list(vararg items: String) = items.joinToString(",", "[", "]")

private fun text(value: String) = value.replace("\\", "\\\\").replace("\"", "\\\"").replace("\n", "\\n")

private fun conversation(id: String, title: String, status: String, node: String, updated: String, agent: String = "claude", cwd: String = "/home/sam/projects/midden", project: String? = null) =
    """{"id":"$id","owner":"u","node_id":"$node","agent":"$agent","cwd":"$cwd","title":"$title","status":"$status","mode":"acceptEdits","model":"Opus 5.5","acp_session_id":"s",
    ${if (project != null) "\"project_id\":\"$project\"," else ""}"created_at":"${ago(days = 2)}","updated_at":"$updated","archived_at":null}"""

private fun chatConversation(id: String, title: String, updated: String) =
    conversation(id, title, "done", "", updated, agent = "chat", cwd = "").replace("\"mode\"", "\"provider_id\":\"mp1\",\"mode\"")

private fun node(id: String, name: String, icon: String, online: Boolean = true, address: String) =
    """{"id":"$id","name":"$name","icon":"$icon","hostname":"$name","os":"linux","arch":"x86_64","version":"0.11.0","address":"$address","online":$online,"draining":false,
    "last_seen_at":"${ago(minutes = 1)}","instructions_revision":"afad776","sessions_on_older_instructions":0,
    "agents":[{"agent":"claude","version":"2.1.308","available":true},{"agent":"codex","version":"0.163.0","available":true}],
    "logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in","usage_left":18,"usage_read_at":"${ago(minutes = 2)}","windows":[{"name":"five_hour","used_percent":82.0,"resets_at":"${ahead(hours = 3)}","rate_per_hour":27.0,"runs_out_at":"${ahead(hours = 1)}"},{"name":"seven_day","used_percent":31.0,"resets_at":"${ahead(days = 3)}","rate_per_hour":0.6}]},{"agent":"codex","account":"sam@example.com","method":"device-auth","state":"signed_in","usage_left":64}]}"""

private val Nodes = list(node(Willow, "willow", "laptop", address = "100.64.4.18"), node(Atlas, "atlas", "server", address = "10.0.0.12"))

private val Conversations = list(
    conversation(Main, "Bump @immich/ui in midden", "working", Willow, ago(minutes = 1), project = "p1"),
    conversation("22222222-2222-2222-2222-222222222222", "Release v0.3.7 for outline", "input", Atlas, ago(minutes = 9), project = "p2"),
    conversation("33333333-3333-3333-3333-333333333333", "Renovate sweep", "failed", Atlas, ago(minutes = 30), project = "p1"),
    conversation("44444444-4444-4444-4444-444444444444", "Restore flow copy", "review", Willow, ago(hours = 1), project = "p1"),
    conversation(Codex, "Uploads survive a 503 and a timeout", "working", Atlas, ago(hours = 2), agent = "codex", project = "p1"),
    conversation("66666666-6666-6666-6666-666666666666", "Flux image automation for vm", "done", Atlas, ago(days = 1), cwd = "/home/sam/projects/vm", project = "p2"),
    conversation("77777777-7777-7777-7777-777777777777", "Dark theme contrast pass", "done", Willow, ago(days = 2), project = "p2"),
    chatConversation(ChatA, "Compare restore strategies", ago(minutes = 4)),
    chatConversation("99999999-9999-9999-9999-99999999999a", "Trip to Lisbon", ago(hours = 3)),
    chatConversation("99999999-9999-9999-9999-99999999999b", "Explain split DNS", ago(days = 3)),
)

private fun event(conversation: String, seq: Int, kind: String, payload: String) = """{"conversation_id":"$conversation","seq":$seq,"kind":"$kind","payload":$payload}"""

private fun update(seq: Int, body: String) = event(Main, seq, "acp_update", """{"sessionId":"s","update":$body}""")

private val MainEvents = list(
    event(Main, 1, "user_message", """{"text":"Bump @immich/ui to the latest release and make sure nothing else in the tree drifts from it."}"""),
    update(2, """{"sessionUpdate":"tool_call","toolCallId":"t1","title":"recall","kind":"other","status":"completed","rawInput":{"command":"@immich/ui version"},"content":[{"type":"content","content":{"type":"text","text":"1 memory"}}]}"""),
    update(3, """{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A **work** memory says to keep `@immich/ui` in step with the `@example/backups-*` packages. They pin 0.31.2, so I'll match that rather than take the newest release."}}"""),
    update(4, """{"sessionUpdate":"tool_call","toolCallId":"t2","title":"Edit package.json","kind":"edit","status":"completed","content":[{"type":"diff","path":"package.json","oldText":"    \"@immich/ui\": \"^0.29.0\",\n    \"kysely\": \"^0.28.2\",","newText":"    \"@immich/ui\": \"0.31.2\",\n    \"kysely\": \"^0.28.2\","}]}"""),
    update(5, """{"sessionUpdate":"tool_call","toolCallId":"t3","title":"pnpm install","kind":"execute","status":"completed","rawInput":{"command":"pnpm install"},"content":[{"type":"content","content":{"type":"text","text":"12s"}}]}"""),
    update(6, """{"sessionUpdate":"tool_call","toolCallId":"t4","title":"pnpm -r test","kind":"execute","status":"in_progress","rawInput":{"command":"pnpm -r test"}}"""),
    event(Main, 7, "permission_request", """{"request_id":"r1","toolCall":{"toolCallId":"t5","title":"git commit -m \"fix(deps): pin @immich/ui to 0.31.2\""},"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"deny","name":"Reject","kind":"reject_once"}]}"""),
)

private val Projects = list(
    """{"id":"p1","node_id":"$Willow","name":"midden","path":"/home/sam/projects/midden","forge_id":"$Github","repository":"example-org/midden","created_at":"${ago(days = 30)}","group_id":"g1"}""",
    """{"id":"p2","node_id":"$Willow","name":"mycelium","path":"/home/sam/projects/mycelium","forge_id":"$Gitlab","repository":"example/mycelium","created_at":"${ago(days = 30)}","group_id":"g2"}""",
    """{"id":"p3","node_id":"$Atlas","name":"homelab","path":"/srv/homelab","forge_id":"$Gitlab","repository":"example/homelab","created_at":"${ago(days = 30)}","group_id":"g3"}""",
)

private val Groups = list(
    """{"id":"g1","name":"work","folders":[],"created_at":"${ago(days = 30)}"}""",
    """{"id":"g2","name":"slop","folders":[],"created_at":"${ago(days = 30)}"}""",
    """{"id":"g3","name":"homelab","folders":[],"created_at":"${ago(days = 30)}"}""",
)

private val Forges = list(
    """{"id":"$Github","kind":"github","base_url":"https://github.com","account":"sam-example","token_hint":"…a1b2","organisations":["example-org","sam-example"],"created_at":"${ago(days = 30)}"}""",
    """{"id":"$Gitlab","kind":"gitlab","base_url":"https://gitlab.example.com","account":"sam","token_hint":"…c3d4","organisations":["example","homelab"],"created_at":"${ago(days = 30)}"}""",
)

private val Repositories = """{"repositories":[
{"forge_id":"$Github","owner":"example-org","name":"midden","full_name":"example-org/midden"},
{"forge_id":"$Gitlab","owner":"example","name":"mycelium","full_name":"example/mycelium"},
{"forge_id":"$Gitlab","owner":"homelab","name":"homelab","full_name":"homelab/homelab"}],
"organisations":[{"forge_id":"$Github","name":"example-org"},{"forge_id":"$Gitlab","name":"example"}]}"""

private val Tasks = list(
    """{"id":"k0","node_id":"$Willow","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Tag v0.11.1 now, or wait for the Firefox fix?","status":"question","options":["Tag now","Wait"],"source_title":"Release","created_at":"${ago(hours = 6)}"}""",
    """{"id":"kp","node_id":"$Willow","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Cache cargo and Gradle in the release jobs","status":"proposed","source_title":"Release","created_at":"${ago(hours = 6)}"}""",
    """{"id":"k1","node_id":"$Willow","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Rebuild install.md for the new flags","status":"open","created_at":"${ago(hours = 5)}"}""",
    """{"id":"k2","node_id":"$Willow","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Verify the release from S3","status":"open","depends_on":["k1"],"created_at":"${ago(hours = 4)}"}""",
    """{"id":"k3","node_id":"$Atlas","agent":"codex","cwd":"/srv","title":"Rotate the S3 keys","status":"open","created_at":"${ago(hours = 3)}"}""",
    """{"id":"kc","node_id":"$Willow","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Merge the three Gradle jobs","status":"claimed","conversation_id":"$Main","created_at":"${ago(hours = 2)}"}""",
    """{"id":"kd1","node_id":"$Willow","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","title":"Pin the Android SDK in CI","status":"done","created_at":"${ago(days = 2)}"}""",
)

private val Schedules = list(
    """{"id":"s1","node_id":"$Willow","agent":"claude","cwd":"/home/sam/projects/midden","project_id":"p1","prompt":"Renovate sweep: update and test dependencies","cron":"0 6 * * 1","each_run":"new","paused":false,"next_run_at":"${ahead(days = 3)}","last_run_at":"${ago(days = 4)}","created_at":"${ago(days = 30)}"}""",
    """{"id":"s2","node_id":"$Willow","agent":"claude","cwd":"/home/sam/projects/midden","project_id":"p1","prompt":"Check backups restored cleanly overnight","cron":"30 7 * * *","each_run":"same","paused":false,"next_run_at":"${ahead(hours = 11)}","last_run_at":"${ago(hours = 2)}","created_at":"${ago(days = 30)}"}""",
    """{"id":"s3","node_id":"$Willow","agent":"claude","cwd":"/home/sam/projects/mycelium","project_id":"p2","prompt":"Summarise what shipped this week","cron":"0 17 * * 5","each_run":"new","paused":true,"next_run_at":"${ahead(days = 4)}","created_at":"${ago(days = 30)}"}""",
)

private val ScheduleRuns = list(
    """{"id":"r1","schedule_id":"s1","ran_at":"${ago(days = 4)}","outcome":"started","conversation_id":"33333333-3333-3333-3333-333333333333"}""",
    """{"id":"r2","schedule_id":"s2","ran_at":"${ago(hours = 2)}","outcome":"started","conversation_id":"66666666-6666-6666-6666-666666666666"}""",
)

private val Artifacts = list(
    """{"id":"a1","owner":"u","conversation_id":"$Main","project_ids":["p2"],"kind":"markdown","title":"Release notes v0.11.0","slug":"release-notes","public":false,"latest_version":2,"created_at":"${ago(days = 2)}","updated_at":"${ago(hours = 3)}"}""",
    """{"id":"a2","owner":"u","conversation_id":"$Main","project_ids":["p2"],"kind":"design","title":"Settings, redesigned","slug":"settings","public":false,"latest_version":4,"created_at":"${ago(days = 4)}","updated_at":"${ago(days = 1)}"}""",
    """{"id":"a3","owner":"u","project_ids":["p1"],"kind":"page","title":"Restore flow walkthrough","slug":"restore","public":true,"latest_version":1,"created_at":"${ago(days = 6)}","updated_at":"${ago(days = 6)}"}""",
    """{"id":"a4","owner":"u","kind":"file","title":"backup-sizes.csv","slug":"sizes","public":false,"latest_version":1,"media_type":"text/csv","created_at":"${ago(days = 8)}","updated_at":"${ago(days = 8)}"}""",
)

private val ReleaseNotes = """# Release notes v0.11.0

## Added

- Wellness and Learning tab with a break reminder
- Queue buttons that never truncate

## Fixed

- Sidebar group filter keeps its place after a restart

| Area | Change |
| --- | --- |
| Chats | Provider picker remembers favourites |
| Tasks | Answers can be edited before sending |
"""

private fun step(key: String, title: String, agent: String, status: String, depends: String = "[]", extra: String = "") =
    """{"key":"$key","title":"$title","agent":"$agent","model":"Opus 5.5","difficulty":"medium","node_id":"$Willow","prompt":"Decide whether to ship.","depends_on":$depends}""" to
        """{"key":"$key","status":"$status","conversation_id":"$Main"$extra}"""

private val Workflows = run {
    val steps = listOf(
        step("a", "Survey the clients", "claude", "done", extra = ""","output":"Three clients share the Shell.""""),
        step("b", "Write the render tests", "claude", "running"),
        step("c", "Audit contrast", "codex", "waiting", depends = """["a"]"""),
        step("d", "Approve the release", "claude", "gate", depends = """["b"]"""),
        step("e", "Build the Android client", "claude", "failed", depends = """["a"]"""),
    )

    list(
        """{"id":"wf1","title":"Release v0.11.1","status":"running","round":2,"parent_id":null,"plan":{"steps":${steps.map { it.first }.joinToString(",", "[", "]")}},"steps":${steps.map { it.second }.joinToString(",", "[", "]")}}""",
    )
}

private val Assistants = list(
    """{"id":"as1","name":"Archivist","description":"Keeps the notes folder tidy and answers questions about it.","group_id":"g1","preferences":{"models":[],"nodes":[],"chat_first":true},"conversation_id":"$ChatA","state":"idle","last_active_at":"${ago(hours = 1)}","session_count":14,"pinned_at":"${ago(days = 3)}","wakes":"every morning at 07:00","created_at":"${ago(days = 20)}","instructions":"Keep notes under 200 lines.","secrets":[{"name":"OUTLINE_TOKEN","description":"Read access to the wiki","state":"set"},{"name":"CALENDAR_KEY","state":"waiting"}]}""",
    """{"id":"as2","name":"Release captain","description":"Tags releases and checks the published artefact.","group_id":"g2","preferences":{"models":[],"nodes":[]},"state":"working","last_active_at":"${ago(minutes = 5)}","session_count":6,"created_at":"${ago(days = 10)}","secrets":[]}""",
)

private val Activity = """{"range":"7d","totals":{"key":"all","name":"All","turns":4212,"reported_turns":4212,"working_ms":900000000,"input_tokens":60000000,"output_tokens":22000000,"cache_read_tokens":90000000,"cache_write_tokens":10000000,"cost_usd":1940.0},
"hours":[${(16..22).flatMap { day -> (0..23).map { day to it } }.joinToString(",") { (day, hour) -> """{"key":"2026-10-${day}T${hour.toString().padStart(2, '0')}","name":"hour","turns":${(day + hour) % 5 + 1},"reported_turns":1,"input_tokens":${(if (hour in 8..20) 900_000 else 150_000) + (day * 37 + hour * 91) % 700_000},"output_tokens":100000}""" }}],
"days":[${(1..7).joinToString(",") { """{"key":"2026-10-${15 + it}","name":"Day $it","turns":${it * 50},"reported_turns":${it * 50},"working_ms":${it * 3_600_000},"input_tokens":${it * 3_000_000},"output_tokens":${it * 1_000_000},"cache_read_tokens":${it * 6_000_000}}""" }}],
"projects":[{"key":"p2","name":"mycelium","turns":900,"reported_turns":900,"input_tokens":30000000,"output_tokens":11000000,"cache_read_tokens":30000000,"cost_usd":760.0},{"key":"p1","name":"midden","turns":700,"reported_turns":700,"input_tokens":20000000,"output_tokens":8000000,"cache_read_tokens":20000000,"cost_usd":510.0}],
"nodes":[{"key":"$Willow","name":"willow","turns":2800,"reported_turns":2800,"input_tokens":50000000,"output_tokens":21000000,"cache_read_tokens":50000000},{"key":"$Atlas","name":"atlas","turns":1400,"reported_turns":1400,"input_tokens":10000000,"output_tokens":1000000,"cache_read_tokens":50000000}]}"""

private val ConversationActivity = list(
    """{"key":"$Main","name":"Bump @immich/ui in midden","turns":42,"reported_turns":42,"working_ms":3600000,"input_tokens":4000000,"output_tokens":900000,"cache_read_tokens":9000000,"cost_usd":31.0}""",
    """{"key":"$Codex","name":"Uploads survive a 503 and a timeout","turns":18,"reported_turns":18,"working_ms":1800000,"input_tokens":2000000,"output_tokens":500000,"cost_usd":12.5}""",
)

private val Resources = """{"latest":{"at_ms":${Clock.System.now().toEpochMilliseconds()},"available":true,"cpu_percent":38.0,"cores":8,"load":[2.1,1.8,1.5],
"memory_total_bytes":34359738368,"memory_available_bytes":11400000000,"swap_total_bytes":0,"swap_free_bytes":0,
"disks":[{"mount":"/","total_bytes":512000000000,"free_bytes":13000000000}],
"processes":[{"pid":10,"name":"gradle daemon","cpu_percent":212.0,"memory_bytes":3300000000,"agent":false,"stoppable":true},{"pid":11,"name":"claude","cpu_percent":14.0,"memory_bytes":670000000,"agent":true,"stoppable":true,"conversation_id":"$Main"},{"pid":12,"name":"postgres","cpu_percent":3.0,"memory_bytes":440000000}],
"ports":[{"port":5173,"address":"127.0.0.1","pid":13,"process":"vite"},{"port":5432,"address":"127.0.0.1","pid":12,"process":"postgres"}],
"docker":{"running":3,"reclaimable_bytes":21800000000,"containers":[${(1..5).joinToString(",") { """{"name":"service-$it","state":"${if (it <= 3) "running" else "exited"}"}""" }}]}},
"history":[${(0..59).joinToString(",") { """{"at_ms":${Clock.System.now().toEpochMilliseconds() - (60 - it) * 10_000},"cpu_percent":${25 + (it * 7) % 30}.0}""" }}]}"""

private val Issues = """{"issues":[
{"forge_id":"$Github","repository":"example-org/midden","number":412,"title":"Restore fails silently after a 503","url":"https://github.example.com/example-org/midden/issues/412","pull_requests":[]},
{"forge_id":"$Github","repository":"example-org/midden","number":398,"title":"Move restore copy into shared strings","url":"https://github.example.com/example-org/midden/issues/398","pull_requests":[{"number":77,"label":"MR !77","url":"https://git.example.com/example-org/midden/-/merge_requests/77","state":"open"}]},
{"forge_id":"$Github","repository":"example-org/midden","number":377,"title":"Android restore progress jumps backwards","url":"https://github.example.com/example-org/midden/issues/377","pull_requests":[]}]}"""

private val Instructions = """{"source":{"forge_id":"$Github","repository":"sam-example/agents-md","path":"AGENTS.md"},"revision":"afad776","layers":[
{"layer":"global","name":"Global","path":"AGENTS.md","text":"## Diffs\n\n- Change only what the task requires.\n- Leave strings that are logged or shown to users alone.\n\n## Testing\n\n- Every code change comes with tests for the behaviour it adds or fixes.","version":{"id":"afad776","created_at":"${ago(hours = 2)}"}},
{"layer":"group:slop","name":"slop","text":""},{"layer":"group:work","name":"work","text":""}]}"""

private val Sessions = list(
    """{"id":"x1","kind":"browser","label":"Firefox on Linux","address":"100.64.4.18","last_used_at":"${ago()}","current":true}""",
    """{"id":"x2","kind":"client","label":"Desktop app on willow","hostname":"willow","address":"100.64.4.18","last_used_at":"${ago(minutes = 12)}"}""",
    """{"id":"x3","kind":"client","label":"Android app on Pixel 9","address":"100.64.7.3","last_used_at":"${ago(hours = 2)}"}""",
)

private val Providers = list(
    """{"id":"mp1","kind":"anthropic","name":"Anthropic","key_hint":"…f00d","models":["Opus 5.5","Sonnet 5.5"],"default_model":"Opus 5.5","created_at":"${ago(days = 30)}"}""",
    """{"id":"mp2","kind":"openai","name":"Ollama","base_url":"http://localhost:11434/v1","models":["llama4"],"default_model":"llama4","created_at":"${ago(days = 30)}"}""",
)

private val Memories = list(
    """{"id":"m1","text":"Keep @immich/ui in step with the backups packages.","scopes":["work"],"created_by":"claude","created_at":"${ago(days = 3)}","updated_at":"${ago(days = 3)}"}""",
    """{"id":"m2","text":"Release tags are cut by the agent, never by hand.","scopes":["slop"],"created_by":"claude","created_at":"${ago(days = 9)}","updated_at":"${ago(days = 9)}"}""",
)

private val Reflection = """{"id":"r1","week":"2026-10-19","done":"Shipped the importer.\n\n- Fixed the retry loop","learned":"Lifetimes finally make sense.","waiting":"Review from Sam","next_week":"Plan the release","edited":["learned"],"drafted_at":"2026-10-23T16:00:00Z"}"""

private val Servers = "[$DocsServer,$MailA,$MailB,$ZulipLocked,$TrackerOff,$StoatExpired]"

private val Calls = list(
    """{"id":"c1","server_id":"o1","server_name":"Docs","tool":"search","kind":"read","conversation_id":"$Main","surface":"conversation","decision":"allowed","at":"${ago(minutes = 20)}"}""",
    """{"id":"c2","server_id":"o1","server_name":"Docs","tool":"create_document","kind":"write","conversation_id":"$Main","surface":"conversation","decision":"asked","review":"approved","at":"${ago(hours = 2)}"}""",
    """{"id":"c3","server_id":"e1","server_name":"Email a","tool":"send_message","kind":"write","conversation_id":"$Codex","surface":"conversation","decision":"asked","review":"declined","at":"${ago(hours = 5)}"}""",
    """{"id":"c4","server_id":"e1","server_name":"Email a","tool":"search_messages","kind":"read","surface":"chat","provider":"Anthropic","decision":"allowed","at":"${ago(days = 1)}"}""",
)

private val Extra = linkedMapOf(
    "/experiments" to EnabledExperiments,
    "/extensions" to """{"generated_ui":true,"rich_markdown":true,"mycelium_tools":true}""",
    "/conversations/$Main/events" to MainEvents,
    "/conversations/$ChatA/events" to list(
        event(ChatA, 1, "user_message", """{"text":"Compare full, incremental and differential restores for 2 TB."}"""),
        event(ChatA, 2, "acp_update", """{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"${text("Full restores are simplest and slowest to prepare, **incremental** chains are fastest to write but slowest to restore, and differential sits between them.\n\n| Strategy | Restore time | Storage |\n| --- | --- | --- |\n| Full | 4.1 h | 2.0 TB |\n| Differential | 6.3 h | 0.6 TB |\n| Incremental | 9.2 h | 0.3 TB |")}"}}}"""),
        event(ChatA, 3, "turn_end", """{"stopReason":"end_turn"}"""),
    ),
    "/artifacts/a1/versions/2/content" to ReleaseNotes,
    "/artifacts" to Artifacts,
    "/conversations" to Conversations,
    "/projects" to Projects,
    "/groups" to Groups,
    "/forges" to Forges,
    "/repositories" to Repositories,
    "/tasks" to Tasks,
    "/schedules" to Schedules,
    "/schedule-runs" to ScheduleRuns,
    "/workflows" to Workflows,
    "/assistants" to Assistants,
    "/activity/conversations" to ConversationActivity,
    "/activity" to Activity,
    "/watch" to Resources,
    "/issues" to Issues,
    "/instructions" to Instructions,
    "/sessions" to Sessions,
    "/model-providers" to Providers,
    "/memories" to Memories,
    "/chat-tool-servers" to Servers,
    "/integration-calls" to Calls,
    "/reflections/schedule" to """{"schedule_id":"s1","node_id":"$Willow","agent":"claude","cwd":"/home/sam/notes","enabled":true,"cron":"0 16 * * 5"}""",
    "/reflections" to "[$Reflection]",
    "/skills" to SkillsJson,
    "/wellness/overview" to WellnessOverviewJson,
    "/wellness/settings" to WellnessSettingsJson,
)

@Composable
private fun Surface(content: @Composable () -> Unit) {
    Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) { content() }
}

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
class AllScreensRenderTest {
    @get:Rule
    val scopes = TestScopes()

    private val me = Me("u", "sam@example.com", "Sam")
    private val appearance = Appearance(true, Accent.Neutral, Accent.entries.filter { it != Accent.System }, {})

    private fun store(extra: Map<String, String> = emptyMap(), open: String? = Main): Store {
        val store = Store(fakeApi(Nodes, linkedMapOf<String, String>().also { merged -> merged.putAll(extra); Extra.forEach { (key, value) -> if (key !in merged) { merged[key] = value } } }), scopes.create())

        runBlocking {
            store.refresh()

            for (load in listOf<suspend () -> Unit>(
                { store.refreshProjects() },
                { store.refreshGroups() },
                { store.refreshRepositories() },
                { store.refreshBoard() },
                { store.refreshSchedules() },
                { store.refreshArtifacts() },
                { store.refreshWorkflows() },
                { store.refreshAssistants() },
                { store.refreshMemoryBanks() },
                { store.learning.loadSkills() },
            )) {
                try {
                    load()
                } catch (_: Exception) {
                }
            }

            if (open != null) {
                store.open(open)
            }
        }

        return store
    }

    private fun snap(name: String, phone: Boolean = false, interact: (ImageComposeScene) -> Unit = {}, content: @Composable () -> Unit) {
        val scene = ImageComposeScene(if (phone) 412 else 1440, if (phone) 915 else 900, Density(1f)) {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalTouch provides phone, LocalAppearance provides appearance, LocalPlatform provides PlatformStub, content = content)
            }
        }

        scene.render(0)
        interact(scene)

        var image = scene.render(200_000_000L)

        for (frame in 1..8) {
            image = scene.render(200_000_000L + frame * 16_000_000L)
        }

        val out = File("build/screens/all/${if (phone) "phone-" else ""}$name.png")
        out.parentFile.mkdirs()
        out.writeBytes(image.encodeToData(EncodedImageFormat.PNG)!!.bytes)
        scene.close()
        assertTrue(out.length() > 1_000, name)
    }

    private fun shell(name: String, route: Route, phone: Boolean = false, extra: Map<String, String> = emptyMap(), open: String? = Main, interact: (ImageComposeScene) -> Unit = {}) {
        val store = store(extra, open)

        snap(name, phone, interact) { Shell(store, PlatformStub, me, onSignOut = {}, initialRoute = route) }
    }

    @Test
    fun shellScreens() {
        shell("shell-empty", Route.Empty, open = null)
        shell("conversation", Route.Chat(Main))
        shell("new-conversation", Route.New, open = null)
        shell("new-chat", Route.NewChat, open = null)
        shell("chat", Route.Chat(ChatA), open = ChatA)
        shell("history", Route.Archive, open = null)
        shell("artefacts", Route.Artifacts, open = null)
        shell("artefact-viewer", Route.Artefact("a1"), open = null)
        shell("tasks", Route.Tasks, open = null)
        shell("workflows", Route.Workflows, open = null)
        shell("schedules", Route.Schedules, open = null)
        shell("assistants", Route.Assistants, open = null)
        shell("assistant-thread", Route.Assistant("as1"), open = null)
        shell("assistant-settings", Route.AssistantSettings("as1"), open = null)
        shell("assistant-new", Route.NewAssistant, open = null)
        shell("wellness", Route.Wellness, open = null)
        shell("projects", Route.Projects, open = null)
        shell("new-in-project", Route.NewIn("p1"), open = null)
        shell("issues", Route.Issues, open = null)
        shell("nodes", Route.Nodes, open = null)
        shell("node-page", Route.Node(Willow), open = null)
        shell("node-resources", Route.Node(Willow), open = null, interact = { click(it, 393f, 84f) })
        shell("node-ports", Route.Node(Willow), open = null, interact = { click(it, 459f, 84f) })
        shell("activity", Route.Activity, open = null)
        shell("settings", Route.Settings, open = null)
    }

    @Test
    fun emptyStates() {
        val none = mapOf("/conversations" to "[]", "/tasks" to "[]", "/schedules" to "[]", "/artifacts" to "[]", "/workflows" to "[]", "/assistants" to "[]", "/projects" to "[]", "/issues" to """{"issues":[]}""", "/nodes" to "[]")

        shell("empty-history", Route.Archive, extra = none, open = null)
        shell("empty-tasks", Route.Tasks, extra = none, open = null)
        shell("empty-schedules", Route.Schedules, extra = none, open = null)
        shell("empty-artefacts", Route.Artifacts, extra = none, open = null)
        shell("empty-workflows", Route.Workflows, extra = none, open = null)
        shell("empty-assistants", Route.Assistants, extra = none, open = null)
        shell("empty-projects", Route.Projects, extra = none, open = null)
        shell("empty-nodes", Route.Nodes, extra = none, open = null)
    }

    @Test
    fun conversationStates() {
        shell("conversation-continue-menu", Route.Chat(Main), interact = { click(it, 1373f, 48f) })
        shell("conversation-teach-me", Route.Chat(Main), interact = { click(it, 532f, 878f) })
        shell("conversation-provider-menu", Route.Chat(Main), interact = { click(it, 937f, 878f) })
        snap("delete-conversation-dialog") { Surface { DeleteConversationDialog("Bump @immich/ui in midden", {}, {}) } }
    }

    @Test
    fun settingsSections() {
        val store = store()

        for (section in SettingsSection.entries) {
            if (section == SettingsSection.ChatModels) {
                continue
            }

            snap("settings-${section.name.lowercase()}") { Surface { SettingsScreen(store, me, "https://mycelium.example.com", startOn = section, onSignOut = {}) } }
        }
    }

    @Test
    fun integrations() {
        val store = store()
        val servers = MyceliumJson.decodeFromString(ListSerializer(ChatToolServer.serializer()), Servers)
        val docs = integrationsOf(servers).first { it.name == "Email" }

        snap("integrations-home") { Surface { ChatToolsSettings(store) } }
        snap("integration-usage") { Surface { IntegrationUsage(store, docs, {}, {}) } }
        snap("integrations-activity") { Surface { IntegrationsActivity(store, integrationsOf(servers), docs, {}, {}) } }
    }

    @Test
    fun phoneScreens() {
        shell("empty", Route.Empty, phone = true, open = null)
        shell("conversation", Route.Chat(Main), phone = true)
        shell("new-conversation", Route.New, phone = true, open = null)
        shell("new-chat", Route.NewChat, phone = true, open = null)
        shell("history", Route.Archive, phone = true, open = null)
        shell("artefacts", Route.Artifacts, phone = true, open = null)
        shell("tasks", Route.Tasks, phone = true, open = null)
        shell("workflows", Route.Workflows, phone = true, open = null)
        shell("schedules", Route.Schedules, phone = true, open = null)
        shell("assistants", Route.Assistants, phone = true, open = null)
        shell("projects", Route.Projects, phone = true, open = null)
        shell("issues", Route.Issues, phone = true, open = null)
        shell("nodes", Route.Nodes, phone = true, open = null)
        shell("activity", Route.Activity, phone = true, open = null)
        shell("wellness", Route.Wellness, phone = true, open = null)
        shell("sidebar-drawer", Route.Empty, phone = true, open = null, interact = { click(it, 28f, 28f) })
    }
}
