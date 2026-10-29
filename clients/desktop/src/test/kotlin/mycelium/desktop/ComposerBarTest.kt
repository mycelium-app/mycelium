package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.runBlocking
import mycelium.Dictation
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.ModelProvider
import mycelium.api.MyceliumApi
import mycelium.state.PickerAccount
import mycelium.state.Choice
import mycelium.state.IdeLauncher
import mycelium.state.SessionOptions
import mycelium.state.Store
import mycelium.ui.BarControl
import mycelium.ui.Composer
import mycelium.ui.AccountPickerControls
import mycelium.ui.ConversationKind
import mycelium.ui.ConversationScreen
import mycelium.ui.GoalControls
import mycelium.ui.HarnessReport
import mycelium.ui.LocalIdes
import mycelium.ui.MyButton
import mycelium.ui.MyIcon
import mycelium.ui.MyceliumTheme
import mycelium.ui.barControls
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val Now = "2026-10-22T10:00:00Z"

private val BarLabels = listOf("Attach files", "Dictate", "Teach me", "Set a goal", "Account: ", "Model: ", "Mode: ", "Conversation usage", "Stop the agent")

private val SilentDictation = object : Dictation {
    override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) = Unit

    override fun stop() = Unit
}

private fun described(prefix: String) = SemanticsMatcher("has a description starting with $prefix") { node ->
    node.config.getOrNull(SemanticsProperties.ContentDescription)?.any { it.startsWith(prefix) } == true
}

private const val ProjectedConversation = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/app","title":"Update every Renovate group to the latest minor and open one merge request per group","status":"done","project_id":"p1","created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:00:00Z"}"""

private const val WillowNode = """[{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.11.0","address":"10.0.0.5","online":true,"draining":false,"sessions":[],"agents":[{"agent":"claude","available":true}],"logins":[]}]"""

private const val HarbourProject = """[{"id":"p1","node_id":"n1","name":"harbour","path":"/work/app","created_at":"2026-10-22T10:00:00Z"}]"""

class ComposerBarTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private fun conversation(agent: String = "claude", status: ConversationStatus = ConversationStatus.Done, scratch: Boolean = false, assistantId: String? = null) =
        Conversation("c1", "u", "n1", agent, "/work", "t", status, scratch = scratch, assistantId = assistantId, createdAt = Now, updatedAt = Now)

    private fun showBar(conversation: Conversation, working: Boolean = false) {
        val chatModel = conversation.agent == "chat"
        val providers = listOf(ModelProvider("p1", "ollama", "Ollama", models = listOf("llama3"), createdAt = Now))
        val accounts = listOf(PickerAccount("account:sam", "claude", "sam@example.com", null, false, ""), PickerAccount("agent:claude:work", "claude", "work@example.com", null, false, ""))

        rule.setContent {
            MyceliumTheme(true) {
                Composer(
                    conversation = conversation, working = working, queue = emptyList(), sending = emptyList(), onSend = {}, onStop = {},
                    onSendQueuedNow = {}, onEditQueued = {}, onRemoveQueued = {},
                    options = SessionOptions(listOf(Choice("default", "Default"), Choice("auto", "Auto")), "auto", listOf(Choice("opus", "Opus 5.5")), "opus", emptyList()),
                    commands = emptyList(), onSetMode = {}, onSetModel = {}, onSetSetting = { _, _ -> }, onSearchFiles = { emptyList() },
                    accountPicker = AccountPickerControls(
                        "Continue on", { _, done -> done(null) },
                        label = if (chatModel) "Ollama · llama3" else "Claude Code · sam@example.com",
                        accounts = accounts, current = "account:sam", providers = providers, providerId = "p1".takeIf { chatModel }, model = "llama3".takeIf { chatModel },
                    ),
                    goal = GoalControls(null, { _, _ -> }, {}, {}, {}, {}),
                    dictation = SilentDictation,
                    onTeachMe = {},
                    usage = { MyButton({}, icon = MyIcon.Gauge, text = "38%", description = "Conversation usage: 38% of the context, $0.42") },
                )
            }
        }
    }

    private fun assertBar(vararg shown: String) {
        val labels = BarLabels + "Send"

        for (label in labels) {
            val matcher = if (label == "Send") described("Send").or(described("Queue message")) else described(label)
            val count = rule.onAllNodes(matcher).fetchSemanticsNodes().size

            assertEquals(if (label in shown) 1 else 0, count, label)
        }
    }

    @Test
    fun aWorkspaceConversationAtRestShowsTheFullBarWithoutStop() {
        showBar(conversation())

        assertBar("Attach files", "Dictate", "Teach me", "Set a goal", "Account: ", "Model: ", "Mode: ", "Conversation usage", "Send")
    }

    @Test
    fun aWorkspaceConversationWorkingAddsStopAndSendQueues() {
        showBar(conversation(status = ConversationStatus.Working), working = true)

        assertBar("Attach files", "Dictate", "Teach me", "Set a goal", "Account: ", "Model: ", "Mode: ", "Conversation usage", "Stop the agent", "Send")
        rule.onNodeWithContentDescription("Queue message").assertExists()
    }

    @Test
    fun aFailedWorkspaceConversationKeepsTheAccountControlSoThePersonCanSwitch() {
        showBar(conversation(status = ConversationStatus.Failed))

        assertBar("Attach files", "Dictate", "Teach me", "Set a goal", "Account: ", "Model: ", "Mode: ", "Conversation usage", "Send")
    }

    @Test
    fun aChatOnAnAgentHasNoGoal() {
        showBar(conversation(scratch = true))

        assertBar("Attach files", "Dictate", "Teach me", "Account: ", "Model: ", "Mode: ", "Conversation usage", "Send")
    }

    @Test
    fun aChatOnAChatModelHasOneModelControlAndItIsTheAccount() {
        showBar(conversation(agent = "chat", scratch = false))

        assertBar("Attach files", "Dictate", "Teach me", "Account: ", "Conversation usage", "Send")
        rule.onNodeWithContentDescription("Account: Ollama · llama3").performClick()
        rule.onNodeWithText("Claude Code · work@example.com").assertExists()
    }

    @Test
    fun anAssistantHasNeitherGoalModelNorMode() {
        showBar(conversation(assistantId = "a1"))

        assertBar("Attach files", "Dictate", "Teach me", "Account: ", "Conversation usage", "Send")
    }

    @Test
    fun theControlsKeepTheDocumentedOrder() {
        showBar(conversation(status = ConversationStatus.Working), working = true)

        val order = listOf("Attach files", "Dictate", "Teach me", "Set a goal", "Account: ", "Model: ", "Mode: ", "Conversation usage", "Stop the agent", "Queue message")
        val lefts = order.map { rule.onNode(described(it)).getBoundsInRoot().left.value }

        assertEquals(lefts.sorted(), lefts)
    }

    @Test
    fun aNewConversationFoldsAgentAndAccountIntoOneControlAndKeepsModel() {
        val everything = HarnessReport(voice = true, working = true, models = true, modes = true, settings = true)

        assertEquals(listOf(BarControl.Attach, BarControl.Project, BarControl.Place, BarControl.Account, BarControl.Model, BarControl.Send), barControls(ConversationKind.NewConversation, everything))
    }

    @Test
    fun teachMeIsOmittedWhereNothingHandlesIt() {
        assertTrue(BarControl.TeachMe !in barControls(ConversationKind.WorkspaceIdle, HarnessReport()))
        assertTrue(BarControl.TeachMe in barControls(ConversationKind.WorkspaceIdle, HarnessReport(teachMe = true)))
    }

    @Test
    fun aNewChatHasOnlyAttachAccountAndSend() {
        val everything = HarnessReport(voice = true, working = true, models = true, modes = true, settings = true)

        assertEquals(listOf(BarControl.Attach, BarControl.Account, BarControl.Send), barControls(ConversationKind.NewChat, everything))
    }

    @Test
    fun anAssistantAlsoGetsItsActions() {
        assertTrue(BarControl.Actions in barControls(ConversationKind.Assistant, HarnessReport()))
        assertTrue(BarControl.Actions !in barControls(ConversationKind.WorkspaceIdle, HarnessReport()))
    }

    private fun showHeader(width: Int) {
        val json = headersOf(HttpHeaders.ContentType, "application/json")
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/conversations") -> "[$ProjectedConversation]"
                path.endsWith("/conversations/c1") -> ProjectedConversation
                path.endsWith("/nodes") -> WillowNode
                path.endsWith("/projects") -> HarbourProject
                path.endsWith("/events") || path.endsWith("/queue") || path.endsWith("/model-providers") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, json)
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())
        val platform = object : Platform by PlatformStub {
            override val opensIdes = true

            override fun hostname(): String = "willow"
        }

        runBlocking {
            store.refresh()
            store.refreshProjects()
            store.open("c1")
        }

        rule.setContent {
            MyceliumTheme(true) {
                val state by store.state.collectAsState()

                CompositionLocalProvider(LocalIdes provides IdeLauncher(store, platform) {}) {
                    Box(Modifier.width(width.dp)) {
                        ConversationScreen(store, state, "c1", onArchived = {}, panels = false)
                    }
                }
            }
        }
        rule.waitForIdle()
    }

    @Test
    fun aWideHeaderShowsProjectNodeAndOpenInIdeOnTheBar() {
        showHeader(1200)

        rule.onNodeWithContentDescription("Project: harbour").assertExists()
        rule.onNodeWithContentDescription("Running on willow, open the node menu").assertExists()
        rule.onNode(hasText("Open in", substring = true)).assertExists()
    }

    @Test
    fun aNarrowHeaderKeepsProjectNodeAndOpenInIdeReachableFromMore() {
        showHeader(420)

        rule.onNodeWithContentDescription("Project: harbour").assertDoesNotExist()
        rule.onNodeWithContentDescription("Running on willow, open the node menu").assertDoesNotExist()
        rule.onNode(hasText("Open in", substring = true)).assertDoesNotExist()

        rule.onNodeWithContentDescription("More").performClick()

        rule.onNodeWithText("Project").assertExists()
        rule.onNodeWithText("harbour").assertExists()
        rule.onNodeWithText("Running on").assertExists()
        rule.onNodeWithText("willow").assertExists()
        rule.onNode(hasText("Open in", substring = true)).assertExists()
    }

    @Test
    fun theNodeInMoreOpensTheNodeMenu() {
        showHeader(420)

        rule.onNodeWithContentDescription("More").performClick()
        rule.onNodeWithText("Running on").performClick()

        rule.onNodeWithText("All nodes").assertExists()
        rule.onNodeWithText("Move").assertExists()
    }
}
