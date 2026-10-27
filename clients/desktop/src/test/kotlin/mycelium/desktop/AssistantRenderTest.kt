package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.state.Store
import mycelium.ui.Accent
import mycelium.ui.Appearance
import mycelium.ui.AssistantSection
import mycelium.ui.LocalAppearance
import mycelium.ui.LocalPlatform
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell
import org.junit.Rule
import java.io.File
import kotlin.test.Test
import kotlin.test.assertTrue

private const val Thread = "77777777-7777-7777-7777-777777777777"
private const val WellnessThread = "88888888-8888-8888-8888-888888888888"
private const val Laptop = "aaaaaaaa-0000-0000-0000-000000000001"

private const val Nodes = """[{"id":"$Laptop","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.11.0","online":true,"draining":false,"agents":[{"agent":"claude","version":"2.1.308","available":true}],"logins":[]}]"""

private const val Groups = """[{"id":"g1","name":"Work","folders":[],"created_at":"2026-10-01T10:00:00Z"},{"id":"g2","name":"Slop","folders":[],"created_at":"2026-10-01T10:00:00Z"}]"""

private const val Archivist = """{"id":"as1","name":"Archivist","description":"Keeps the notes folder tidy and answers questions about it.","group_id":"g1","preferences":{"models":[{"agent":"claude"}],"nodes":["$Laptop"],"chat_first":true},"conversation_id":"$Thread","state":"idle","last_active_at":"2026-10-27T08:00:00Z","session_count":14,"wakes":"every morning at 07:00","created_at":"2026-10-01T10:00:00Z","instructions":"Keep notes under 200 lines.\n\nFile new notes under the folder named for the month.","secrets":[{"name":"OUTLINE_TOKEN","description":"Read access to the wiki","state":"set"},{"name":"CALENDAR_KEY","state":"waiting"}]}"""

private const val Wellness = """{"id":"wl","name":"Wellness","description":"Drafts reflections, end-of-day wrap-ups and exercise reviews.","preferences":{"models":[{"agent":"claude"}],"nodes":[]},"conversation_id":"$WellnessThread","state":"idle","last_active_at":"2026-10-27T08:00:00Z","session_count":3,"wakes":"Fridays at 16:00 and weekdays at 17:30","builtin_key":"wellness","created_at":"2026-10-01T10:00:00Z","secrets":[]}"""

private const val Captain = """{"id":"as2","name":"Release captain","description":"Tags releases and checks the published artefact.","group_id":"g2","preferences":{"models":[],"nodes":[]},"state":"working","last_active_at":"2026-10-27T09:00:00Z","session_count":6,"created_at":"2026-10-10T10:00:00Z","secrets":[]}"""

private const val ArchivistActions = """[
{"id":"x1","assistant_id":"as1","name":"Daily summary","prompt":"Summarise what changed in the notes folder yesterday.","inputs":[],"pinned":true,"position":0},
{"id":"x2","assistant_id":"as1","name":"Meeting notes","prompt":"Write up the notes for {meeting} on {date}.","inputs":[{"name":"meeting","kind":"text","required":true},{"name":"date","kind":"date","required":true,"default":"today"}],"pinned":true,"position":1},
{"id":"x3","assistant_id":"as1","name":"Tidy inbox","prompt":"File everything in the inbox folder.","inputs":[],"model":{"agent":"claude"},"pinned":false,"position":2}
]"""

private const val WellnessActions = """[
{"id":"w1","assistant_id":"wl","name":"Weekly reflection","prompt":"Draft the reflection for {week}.","inputs":[{"name":"week","kind":"text","required":true}],"pinned":false,"position":0,"builtin_key":"weekly_reflection"},
{"id":"w2","assistant_id":"wl","name":"End-of-day wrap-up","prompt":"Wrap up today.","inputs":[],"pinned":true,"position":1,"builtin_key":"end_of_day"},
{"id":"w3","assistant_id":"wl","name":"Review an exercise","prompt":"Review {exercise}.","inputs":[{"name":"exercise","kind":"text","required":true}],"pinned":false,"position":2,"builtin_key":"exercise_review"}
]"""

private const val Proposals = """[{"id":"pp1","assistant_id":"as1","conversation_id":"$Thread","name":"Weekly digest","prompt":"List what changed this week, grouped by folder.","inputs":[{"name":"week","kind":"text","required":false}],"reason":"You asked for a weekly summary three times this month.","state":"waiting","created_at":"2026-10-27T09:00:00Z"}]"""

private const val Changes = """[
{"id":"ch1","conversation_id":"$Thread","kind":"create","name":"Reviewer","description":"Reads merge requests and leaves comments.","group_id":"g1","instructions":"Review each merge request for correctness first, then style.","state":"waiting","created_at":"2026-10-27T09:00:00Z"},
{"id":"ch2","conversation_id":"$Thread","kind":"update","assistant_id":"as1","name":"Archivist","description":"Keeps the notes folder tidy.","state":"applied","created_at":"2026-10-27T09:01:00Z"}
]"""

private const val Schedules = """[
{"id":"s1","node_id":"$Laptop","agent":"claude","cwd":"","assistant_id":"as1","action_id":"x1","prompt":"","cron":"0 7 * * *","each_run":"same","paused":false,"next_run_at":"2026-10-28T07:00:00Z","created_at":"2026-10-01T10:00:00Z"}
]"""

private val Thread1 = """{"id":"$Thread","owner":"u","node_id":"$Laptop","agent":"chat","cwd":"","title":"Archivist","status":"review","assistant_id":"as1","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-27T09:00:00Z"}"""

private val Thread2 = """{"id":"$WellnessThread","owner":"u","node_id":"$Laptop","agent":"chat","cwd":"","title":"Wellness","status":"review","assistant_id":"wl","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-27T09:00:00Z"}"""

private const val Reflection = """{"id":"r1","week_start":"2026-10-19","status":"draft","body":"A steady week.","created_at":"2026-10-24T16:00:00Z","updated_at":"2026-10-24T16:00:00Z"}"""

class AssistantRenderTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val me = Me("u", "sam@example.com", "Sam")
    private val appearance = Appearance(true, Accent.Neutral, Accent.entries.filter { it != Accent.System }, {})

    private fun api(): MyceliumApi {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val routes = linkedMapOf(
            "/assistants/as1/actions" to ArchivistActions,
            "/assistants/wl/actions" to WellnessActions,
            "/assistants/as1/action-proposals" to Proposals,
            "/assistants/as1/files" to """[{"path":"AGENTS.md","size":60,"updated_at":"2026-10-22T10:00:00Z"}]""",
            "/assistants/as1/secrets" to """[{"name":"OUTLINE_TOKEN","description":"Read access to the wiki","state":"set"},{"name":"CALENDAR_KEY","state":"waiting"}]""",
            "/assistants/as1" to Archivist,
            "/assistants/wl/files" to "[]",
            "/assistants/wl/secrets" to "[]",
            "/assistants/wl" to Wellness,
            "/assistants/builtin/wellness" to Wellness,
            "/assistants" to "[$Archivist,$Wellness,$Captain]",
            "/assistant-changes" to Changes,
            "/groups" to Groups,
            "/nodes" to Nodes,
            "/schedules" to Schedules,
            "/model-providers" to """[{"id":"mp1","kind":"anthropic","name":"Anthropic","key_hint":"f00d","models":["Opus 5.5","Sonnet 5.5"],"default_model":"Opus 5.5","created_at":"2026-10-01T10:00:00Z"}]""",
            "/experiments" to EnabledExperiments,
            "/conversations/$Thread" to Thread1,
            "/conversations/$WellnessThread" to Thread2,
            "/conversations" to "[$Thread1,$Thread2]",
            "/events" to "[]",
            "/queue" to "[]",
            "/reflections" to "[$Reflection]",
            "/instruction-proposals" to "[]",
        )
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = routes.entries.firstOrNull { path.endsWith(it.key) }?.value ?: "{}"

            respond(body, HttpStatusCode.OK, json)
        }

        return MyceliumApi("https://mycelium.test", "token", engine)
    }

    private fun show(route: Route): Store {
        val store = Store(api(), scopes.create())

        runBlocking {
            for (load in listOf<suspend () -> Unit>({ store.refresh() }, { store.refreshAssistants() }, { store.refreshGroups() }, { store.refreshSchedules() })) {
                try {
                    load()
                } catch (_: Exception) {
                }
            }
        }

        rule.setContent {
            MyceliumTheme(true) {
                CompositionLocalProvider(LocalAppearance provides appearance, LocalPlatform provides PlatformStub) {
                    Box(Modifier.size(1440.dp, 900.dp)) {
                        Shell(store, PlatformStub, me, onSignOut = {}, initialRoute = route)
                    }
                }
            }
        }

        return store
    }

    private fun snap(name: String) {
        rule.waitForIdle()

        val roots = rule.onAllNodes(isRoot())

        roots.fetchSemanticsNodes().indices.forEach { index ->
            val png = org.jetbrains.skia.Image.makeFromBitmap(roots[index].captureToImage().asSkiaBitmap()).encodeToData(org.jetbrains.skia.EncodedImageFormat.PNG)!!.bytes
            val out = File("build/screens/assistants/$name${if (index == 0) "" else "-layer$index"}.png")
            out.parentFile.mkdirs()
            out.writeBytes(png)
            assertTrue(png.size > 1_000, name)
        }
    }

    private fun waitFor(text: String) {
        rule.waitUntil(5_000) { rule.onAllNodesWithText(text, substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theAssistantsListShowsWellnessAsBuiltIn() {
        show(Route.Assistants)
        waitFor("Built in")
        snap("7.1-assistants")
    }

    @Test
    fun theNewAssistantDialogAsksForNameGroupAndInstructions() {
        show(Route.NewAssistant)
        waitFor("Create assistant")
        snap("7.4-new-assistant")
    }

    @Test
    fun identitySettings() {
        show(Route.AssistantSettings("as1"))
        waitFor("Archive assistant")
        snap("7.3-identity")
    }

    @Test
    fun instructionsSettings() {
        show(Route.AssistantSettings("as1", AssistantSection.Instructions))
        waitFor("Proposed edits")
        snap("7.7-instructions")
    }

    @Test
    fun modelsSettings() {
        show(Route.AssistantSettings("as1", AssistantSection.ModelsAndNodes))
        waitFor("Runs on, in order of preference")
        snap("7.8-models")
    }

    @Test
    fun storageSettings() {
        show(Route.AssistantSettings("as1", AssistantSection.StorageAndSecrets))
        waitFor("OUTLINE_TOKEN")
        snap("7.9-storage")
    }

    @Test
    fun schedulesAndActionsSettings() {
        show(Route.AssistantSettings("as1", AssistantSection.SchedulesAndActions))
        waitFor("Meeting notes")
        snap("7.10-schedules-and-actions")
    }

    @Test
    fun theActionDialog() {
        show(Route.AssistantSettings("as1", AssistantSection.SchedulesAndActions))
        waitFor("New action")
        rule.onNodeWithText("New action").performClick()
        waitFor("Save action")
        snap("7.11-action-dialog")
    }

    @Test
    fun theScheduleDialogForAnAssistant() {
        show(Route.AssistantSettings("as1", AssistantSection.SchedulesAndActions))
        waitFor("New schedule")
        rule.onNodeWithText("New schedule").performClick()
        waitFor("Cancel")
        snap("7.14-schedule-dialog")
    }

    @Test
    fun theThreadShowsPinnedActionsAndProposals() {
        show(Route.Assistant("as1"))
        waitFor("Proposed an action")
        snap("7.2-thread")
    }

    @Test
    fun theActionsMenu() {
        show(Route.Assistant("as1"))
        waitFor("Proposed an action")
        rule.onNodeWithContentDescription("Actions").performClick()
        waitFor("Manage actions")
        snap("7.12-actions-menu")
    }

    @Test
    fun runningAnActionThatAsksForInputs() {
        show(Route.Assistant("as1"))
        waitFor("Proposed an action")
        rule.onNodeWithContentDescription("Run Meeting notes").performClick()
        waitFor("Sends")
        snap("7.13-run-action")
    }

    @Test
    fun agentAssistantChangesAppearAsCards() {
        show(Route.Assistant("as1"))
        waitFor("Proposed an assistant")
        snap("7.16-assistant-changes")
    }

    @Test
    fun theWellnessSettingsListItsBuiltInActionsAndSchedules() {
        show(Route.AssistantSettings("wl", AssistantSection.SchedulesAndActions))
        waitFor("Weekly reflection")
        snap("7.17-wellness")
    }

    @Test
    fun theWellnessIdentityExplainsItIsBuiltIn() {
        show(Route.AssistantSettings("wl"))
        waitFor("Wellness is built in")
        snap("7.17-wellness-identity")
    }
}
