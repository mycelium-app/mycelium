package mycelium

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import mycelium.api.Conversation
import mycelium.api.ConversationStatus
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.MyceliumJson
import mycelium.state.Choice
import mycelium.state.Mention
import mycelium.state.SlashCommand
import mycelium.state.mentionAt
import mycelium.state.sessionOptions
import mycelium.state.slashCommands
import kotlin.test.Test
import kotlin.test.assertEquals

private fun conversation(options: String?, model: String? = null) = Conversation(
    id = "c1",
    owner = "u",
    nodeId = "n",
    agent = "claude",
    cwd = "/work",
    title = "t",
    status = ConversationStatus.Done,
    model = model,
    options = options?.let { MyceliumJson.parseToJsonElement(it).jsonObject },
    createdAt = "2026-10-22T10:00:00Z",
    updatedAt = "2026-10-22T10:00:00Z",
)

private fun update(seq: Long, body: String) = Event("c1", seq, EventKinds.AcpUpdate, MyceliumJson.parseToJsonElement("""{"sessionId":"s","update":$body}"""))

class SessionTest {
    private val offered = """
        {"modes": {"currentModeId": "default", "availableModes": [{"id": "default", "name": "Default"}, {"id": "plan", "name": "Plan"}]},
         "models": {"currentModelId": "opus", "availableModels": [{"modelId": "opus", "name": "Opus 5.5"}, {"modelId": "sonnet", "name": "Sonnet 5.5", "description": "Fast"}]},
         "config_options": [{"id": "effort", "name": "Effort", "type": "select", "currentValue": "medium",
                             "options": [{"value": "low", "name": "Low"}, {"value": "medium", "name": "Medium"}]}]}
    """.trimIndent()

    @Test
    fun theOfferedModesModelsAndSettingsAreReadFromTheConversation() {
        val options = sessionOptions(conversation(offered), emptyList())

        assertEquals(listOf(Choice("default", "Default"), Choice("plan", "Plan")), options.modes)
        assertEquals("default", options.currentMode)
        assertEquals(listOf(Choice("opus", "Opus 5.5"), Choice("sonnet", "Sonnet 5.5", "Fast")), options.models)
        assertEquals("opus", options.currentModel)
        assertEquals("medium", options.settings.single().current)
        assertEquals(listOf("low", "medium"), options.settings.single().choices.map { it.id })
    }

    @Test
    fun laterSessionUpdatesChangeTheCurrentModeAndSettings() {
        val events = listOf(
            update(1, """{"sessionUpdate": "current_mode_update", "currentModeId": "plan"}"""),
            update(2, """{"sessionUpdate": "config_option_update", "configOptions": [{"id": "effort", "name": "Effort", "currentValue": "high", "options": [{"group": "g", "name": "All", "options": [{"value": "high", "name": "High"}]}]}]}"""),
        )

        val options = sessionOptions(conversation(offered), events)

        assertEquals("plan", options.currentMode)
        assertEquals("high", options.settings.single().current)
        assertEquals(listOf(Choice("high", "High")), options.settings.single().choices)
    }

    @Test
    fun aConversationWithoutOfferedOptionsHasNone() {
        val options = sessionOptions(conversation(null, model = "opus"), emptyList())

        assertEquals(emptyList(), options.models)
        assertEquals("opus", options.currentModel)
    }

    @Test
    fun theLatestAdvertisedCommandsAreTheOnesOffered() {
        val events = listOf(
            update(1, """{"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "old", "description": "gone"}]}"""),
            update(2, """{"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "review", "description": "Review the diff", "input": {"hint": "branch"}}]}"""),
        )

        assertEquals(listOf(SlashCommand("review", "Review the diff", "branch")), slashCommands(events))
    }

    @Test
    fun stateKeptByTheControlPlaneStandsInWhenTheEarlyEventsAreNotLoaded() {
        val kept = listOf(
            MyceliumJson.parseToJsonElement("""{"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "review", "description": "Review the diff"}]}""").jsonObject,
            MyceliumJson.parseToJsonElement("""{"sessionUpdate": "current_mode_update", "currentModeId": "plan"}""").jsonObject,
        )

        assertEquals(listOf(SlashCommand("review", "Review the diff", null)), slashCommands(emptyList(), kept))
        assertEquals("plan", sessionOptions(conversation(offered), emptyList(), kept).currentMode)
    }

    @Test
    fun liveEventsOverrideTheStateKeptByTheControlPlane() {
        val kept = listOf(
            MyceliumJson.parseToJsonElement("""{"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "review", "description": "Review the diff"}]}""").jsonObject,
            MyceliumJson.parseToJsonElement("""{"sessionUpdate": "current_mode_update", "currentModeId": "plan"}""").jsonObject,
        )
        val events = listOf(
            update(9, """{"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "compact", "description": "Shrink"}]}"""),
            update(10, """{"sessionUpdate": "current_mode_update", "currentModeId": "default"}"""),
        )

        assertEquals(listOf(SlashCommand("compact", "Shrink", null)), slashCommands(events, kept))
        assertEquals("default", sessionOptions(conversation(offered), events, kept).currentMode)
    }

    @Test
    fun theLatestOfSeveralUpdatesWinsAndKeptValuesApplyOnlyWhenEventsHoldNone() {
        val kept = listOf(MyceliumJson.parseToJsonElement("""{"sessionUpdate": "current_mode_update", "currentModeId": "plan"}""").jsonObject)
        val events = listOf(
            update(1, """{"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "old", "description": "Old"}]}"""),
            update(2, """{"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "new", "description": "New"}]}"""),
        )

        assertEquals(listOf(SlashCommand("new", "New", null)), slashCommands(events))
        assertEquals("plan", sessionOptions(conversation(offered), events, kept).currentMode)
    }
}

class MentionTest {
    @Test
    fun anAtSignStartingAWordBeforeTheCursorIsAMention() {
        assertEquals(Mention(6, "fla"), mentionAt("check @fla", 10))
    }

    @Test
    fun anEmailAddressOrAFinishedWordIsNotAMention() {
        assertEquals(null, mentionAt("mail me@host", 12))
        assertEquals(null, mentionAt("@flake.nix and", 14))
    }
}
