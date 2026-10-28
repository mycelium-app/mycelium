package mycelium

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import mycelium.api.EnvironmentApp
import mycelium.api.EnvironmentAppKind
import mycelium.api.EnvironmentCapability
import mycelium.api.EnvironmentController
import mycelium.api.EnvironmentState
import mycelium.api.EnvironmentVersions
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.buildThread
import mycelium.state.ThreadItem
import mycelium.api.EventKinds
import mycelium.api.Event
import kotlinx.serialization.json.Json
import mycelium.api.NodeSummary
import mycelium.api.StreamMessage
import mycelium.api.parseStreamMessage
import mycelium.state.AppState
import mycelium.state.EnvironmentStage
import mycelium.state.environmentActionAge
import mycelium.state.environmentCannotRunText
import mycelium.state.environmentControl
import mycelium.state.environmentMissingText
import mycelium.state.environmentRunningFor
import mycelium.state.EnvironmentNoNodeText
import mycelium.state.environmentNodes
import mycelium.state.environmentRunOn
import mycelium.state.environmentStage
import mycelium.state.environmentStoppedText
import mycelium.state.environmentAppsToStart
import mycelium.state.environmentCurrentWindow
import mycelium.state.environmentDefaultApps
import mycelium.state.environmentOpenApps
import mycelium.state.environmentSizeText
import mycelium.state.environmentWindowApp
import mycelium.state.environmentVersionsText
import mycelium.state.environmentViewUrl
import mycelium.state.nodeThatCanRunEnvironment
import mycelium.state.reduce
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.time.Instant

private const val EnvironmentJson = """{"type":"environment","conversation_id":"c1","state":"running","controller":"agent","node_id":"n1","node_name":"atlas-box","size":{"width":1280,"height":800},"started_at":"2026-10-28T10:00:00Z","browser":"Chromium 141","target":{"x":640,"y":400,"label":"Click, Password"}}"""

private fun environment(state: EnvironmentState = EnvironmentState.Running, controller: EnvironmentController = EnvironmentController.Agent) =
    (parseStreamMessage(EnvironmentJson) as StreamMessage.EnvironmentChanged).environment.copy(state = state, controller = controller)

private fun node(id: String, name: String, environment: EnvironmentCapability?, online: Boolean = true) = MyceliumJson.decodeFromString(
    NodeSummary.serializer(),
    """{"id":"$id","name":"$name","hostname":"$name","os":"linux","arch":"x86_64","version":"v0.14.0","online":$online,"draining":false}""",
).copy(environment = environment)

private val CanRun = EnvironmentCapability(true, versions = EnvironmentVersions("sway 1.10", "0.9.1", "Chromium 141"))
private val CannotRun = EnvironmentCapability(false, missing = listOf("sway", "wayvnc"))

class EnvironmentsTest {
    @Test
    fun anEnvironmentMessageCarriesTheSessionAtTheMessageLevel() {
        val message = parseStreamMessage(EnvironmentJson)

        assertIs<StreamMessage.EnvironmentChanged>(message)
        assertEquals("atlas-box", message.environment.nodeName)
        assertEquals(1280, message.environment.size.width)
        assertEquals("Click, Password", message.environment.target?.label)
    }

    @Test
    fun anEnvironmentActionMessageNamesTheConversationAndTheAction() {
        val message = parseStreamMessage("""{"type":"environment_action","conversation_id":"c1","action":{"at":"2026-10-28T10:00:05Z","label":"Clicking “Password”"}}""")

        assertIs<StreamMessage.EnvironmentActed>(message)
        assertEquals("c1", message.conversationId)
        assertEquals("Clicking “Password”", message.action.label)
    }

    @Test
    fun theReducerKeepsEnvironmentsByConversationAndActionsNewestFirst() {
        val running = environment()
        val first = parseStreamMessage("""{"type":"environment_action","conversation_id":"c1","action":{"at":"2026-10-28T10:00:05Z","label":"Took a screenshot"}}""") as StreamMessage.EnvironmentActed
        val second = parseStreamMessage("""{"type":"environment_action","conversation_id":"c1","action":{"at":"2026-10-28T10:00:09Z","label":"Clicked “Email”"}}""") as StreamMessage.EnvironmentActed

        val state = listOf(StreamMessage.EnvironmentChanged(running), first, second).fold(AppState(), ::reduce)

        assertEquals(running, state.environments["c1"])
        assertEquals(listOf("Clicked “Email”", "Took a screenshot"), state.environmentActions["c1"]?.map { it.label })
    }

    @Test
    fun theReducerReplacesAnEnvironmentWhenItsControllerChanges() {
        val state = listOf(environment(), environment(controller = EnvironmentController.Person)).fold(AppState()) { held, next -> reduce(held, StreamMessage.EnvironmentChanged(next)) }

        assertEquals(EnvironmentController.Person, state.environments["c1"]?.controller)
    }

    @Test
    fun theStageFollowsTheSessionAndFallsBackToWhatTheNodeCanRun() {
        assertEquals(EnvironmentStage.Empty, environmentStage(null, CanRun))
        assertEquals(EnvironmentStage.Empty, environmentStage(null, null))
        assertEquals(EnvironmentStage.CannotRun, environmentStage(null, CannotRun))
        assertEquals(EnvironmentStage.Starting, environmentStage(environment(EnvironmentState.Starting), CanRun))
        assertEquals(EnvironmentStage.Active, environmentStage(environment(), CanRun))
        assertEquals(EnvironmentStage.Failed, environmentStage(environment(EnvironmentState.Failed), CanRun))
        assertEquals(EnvironmentStage.Stopped, environmentStage(environment(EnvironmentState.Stopped), CanRun))
        assertEquals(EnvironmentStage.CannotRun, environmentStage(environment(EnvironmentState.Stopped), CannotRun))
    }

    @Test
    fun theStoppedSentenceNamesTheLocalTimeAndWarnsAboutSignedInSessions() {
        val stopped = environment(EnvironmentState.Stopped).copy(stoppedAt = "2026-10-28T14:32:10Z")

        assertEquals(
            "You stopped it at 14:32. The browser closed with it, so anything signed in there is signed out.",
            environmentStoppedText(stopped) { "2026-10-28T14:32:10" },
        )
    }

    @Test
    fun anEnvironmentStoppedWithItsConversationSaysSoAndNamesTheTime() {
        val stopped = environment(EnvironmentState.Stopped).copy(stoppedAt = "2026-10-28T15:10:10Z", closedWithConversation = true)

        assertEquals("It stopped when you closed this conversation at 15:10, so the apps in it closed too.", environmentStoppedText(stopped) { "2026-10-28T15:10:10" })
    }

    @Test
    fun theEnvironmentLinesOfAThreadAreShownAsNotes() {
        val thread = buildThread(listOf(Event("c1", 1, EventKinds.Environment, Json.parseToJsonElement("{\"text\":\"You handed back the environment.\"}"))))

        assertEquals(ThreadItem.Note("e1", "You handed back the environment."), thread.single())
    }

    @Test
    fun theMissingToolsSentenceNamesEveryMissingToolAndTheNode() {
        assertEquals("sway and wayvnc are not installed on laptop", environmentMissingText(listOf("sway", "wayvnc"), "laptop"))
        assertEquals("wayvnc is not installed on laptop", environmentMissingText(listOf("wayvnc"), "laptop"))
        assertEquals("sway, wayvnc and chromium are not installed on laptop", environmentMissingText(listOf("sway", "wayvnc", "chromium"), "laptop"))
    }

    @Test
    fun aNodeThatCannotRunEnvironmentsSaysWhyFromTheMissingToolsOrTheNote() {
        assertEquals("Cannot run environments: sway and wayvnc are not installed.", environmentCannotRunText(CannotRun))
        assertEquals("Environments need a Linux node", environmentCannotRunText(EnvironmentCapability(false, note = "Environments need a Linux node")))
    }

    @Test
    fun theVersionsReadAsTheToolsTheNodeFound() {
        assertEquals("sway 1.10, wayvnc 0.9.1, Chromium 141", environmentVersionsText(CanRun.versions))
    }

    @Test
    fun theSummaryNamesTheSessionSizeAndBrowser() {
        assertEquals("1280 × 800", environmentSizeText(environment()))
    }

    @Test
    fun actionTimesCountSecondsThenFallBackToMinutes() {
        val now = Instant.parse("2026-10-28T10:05:00Z")

        assertEquals("now", environmentActionAge("2026-10-28T10:04:58Z", now))
        assertEquals("8s", environmentActionAge("2026-10-28T10:04:52Z", now))
        assertEquals("3m", environmentActionAge("2026-10-28T10:02:00Z", now))
    }

    @Test
    fun runningForReadsInMinutesThenHours() {
        val now = Instant.parse("2026-10-28T12:00:00Z")

        assertEquals("12 min", environmentRunningFor("2026-10-28T11:48:00Z", now))
        assertEquals("1 h", environmentRunningFor("2026-10-28T11:00:00Z", now))
    }

    @Test
    fun theControlColumnNamesWhoHoldsTheEnvironment() {
        assertEquals("Agent in control", environmentControl(EnvironmentController.Agent, null))
        assertEquals("Sam in control", environmentControl(EnvironmentController.Person, "Sam"))
    }

    @Test
    fun theOtherNodePickerSkipsThisNodeAndNodesThatCannotRunOne() {
        val nodes = listOf(
            node("n1", "laptop", CannotRun),
            node("n2", "zeta", CanRun),
            node("n3", "atlas-box", CanRun),
            node("n4", "offline-box", CanRun, online = false),
            node("n5", "plain", null),
        )

        assertEquals("atlas-box", nodeThatCanRunEnvironment(nodes, "n1")?.name)
        assertNull(nodeThatCanRunEnvironment(nodes.filter { it.id in setOf("n1", "n5") }, "n1"))
    }

    @Test
    fun theViewUrlCarriesAOneUseTicketAndNeverTheToken() {
        val url = environmentViewUrl("https://mycelium.test/", "c1", "t1")

        assertEquals("https://mycelium.test/novnc/embed.html?path=%2Fapi%2Fv1%2Fconversations%2Fc1%2Fenvironment%2Fstream&ticket=t1", url)
    }

    @Test
    fun startingTakeOverAndHandBackSendTheRightRequests() = runTest {
        val seen = mutableListOf<String>()
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            seen += "${request.method.value} ${request.url.encodedPath}"

            when {
                request.method == HttpMethod.Delete -> respond("", HttpStatusCode.NoContent)
                request.url.encodedPath.endsWith("/actions") -> respond("""[{"at":"2026-10-28T10:00:05Z","label":"Took a screenshot"}]""", HttpStatusCode.OK, json)
                request.url.encodedPath.endsWith("/check") -> respond("", HttpStatusCode.OK, json)
                else -> respond(EnvironmentJson, HttpStatusCode.OK, json)
            }
        }
        val api = MyceliumApi("https://mycelium.test", "token", engine)

        api.startEnvironment("c1")
        api.takeOverEnvironment("c1")
        api.handBackEnvironment("c1")
        api.stopEnvironment("c1")

        assertEquals(
            listOf(
                "POST /api/v1/conversations/c1/environment",
                "POST /api/v1/conversations/c1/environment/take-over",
                "POST /api/v1/conversations/c1/environment/hand-back",
                "DELETE /api/v1/conversations/c1/environment",
            ),
            seen,
        )
        assertNull(api.checkEnvironment("n1"))
        assertEquals("Took a screenshot", api.environmentActions("c1").single().label)
    }

    @Test
    fun noEnvironmentIsNullRatherThanAnError() = runTest {
        val engine = MockEngine { respond("""{"error":"not_found","message":"No environment."}""", HttpStatusCode.NotFound, headersOf(HttpHeaders.ContentType, "application/json")) }

        assertNull(MyceliumApi("https://mycelium.test", "token", engine).environment("c1"))
    }

    @Test
    fun theFirstBrowserIsTickedByDefaultAndNothingElse() {
        val apps = listOf(App("files", EnvironmentAppKind.FileManager), App("firefox", EnvironmentAppKind.Browser), App("chromium", EnvironmentAppKind.Browser))

        assertEquals(setOf("firefox"), environmentDefaultApps(apps))
        assertEquals(emptySet(), environmentDefaultApps(apps.take(1)))
    }

    @Test
    fun theAppsToStartKeepTheOrderTheNodeOffersThem() {
        val apps = listOf(App("files", EnvironmentAppKind.FileManager), App("chromium", EnvironmentAppKind.Browser))

        assertEquals(listOf("files", "chromium"), environmentAppsToStart(apps, setOf("chromium", "files")))
        assertEquals(emptyList(), environmentAppsToStart(apps, emptySet()))
    }

    @Test
    fun anEnvironmentMessageCarriesItsWindowsAndMarksTheAgentsWindow() {
        val windows = """"windows":[{"id":4,"app_id":"chromium","title":"Inbox","focused":false},{"id":9,"app_id":"files","title":"Downloads","focused":true}],"agent_window":4,"""
        val message = parseStreamMessage(EnvironmentJson.replace(""""browser"""", windows + """"browser""""))
        assertIs<StreamMessage.EnvironmentChanged>(message)
        val apps = listOf(App("chromium", EnvironmentAppKind.Browser), App("files", EnvironmentAppKind.FileManager))
        val environment = message.environment

        assertEquals("Downloads", environmentCurrentWindow(environment)?.title)
        assertEquals("Chromium · agent", environmentWindowApp(environment, environment.windows[0], apps))
        assertEquals("Files", environmentWindowApp(environment, environment.windows[1], apps))
        assertEquals("Chromium, Files", environmentOpenApps(environment, apps))
    }

    @Test
    fun onlyOnlineNodesThatCanRunAnEnvironmentAreOfferedToRunOn() {
        val nodes = listOf(node("n1", "laptop", CanRun), node("n2", "atlas-box", CanRun), node("n3", "old-box", CannotRun), node("n4", "asleep", CanRun, online = false), node("n5", "unchecked", null))

        assertEquals(listOf("atlas-box", "laptop"), environmentNodes(nodes).map { it.name })
    }

    @Test
    fun runOnDefaultsToTheNodeUsedLastThenTheFirstAndTheChoiceWins() {
        val eligible = environmentNodes(listOf(node("n1", "laptop", CanRun), node("n2", "atlas-box", CanRun)))

        assertEquals("laptop", environmentRunOn(eligible, lastUsedId = "n1", chosenId = null)?.name)
        assertEquals("atlas-box", environmentRunOn(eligible, lastUsedId = "n1", chosenId = "n2")?.name)
        assertEquals("atlas-box", environmentRunOn(eligible, lastUsedId = "gone", chosenId = null)?.name)
    }

    @Test
    fun aChatWithNoNodeOnlineShowsTheNoNodeStateUntilOneComesOnline() {
        assertEquals(EnvironmentStage.NoNode, environmentStage(null, null, noNode = true))
        assertEquals(EnvironmentStage.NoNode, environmentStage(environment(EnvironmentState.Stopped), null, noNode = true))
        assertEquals(EnvironmentStage.Active, environmentStage(environment(), null, noNode = true))
        assertEquals(EnvironmentStage.Empty, environmentStage(null, CanRun, noNode = false))
        assertEquals("No node that can run an environment is online. Start one from Nodes.", EnvironmentNoNodeText)
    }
}

private fun App(id: String, kind: EnvironmentAppKind) = EnvironmentApp(id, id.replaceFirstChar { it.uppercase() }, kind)
