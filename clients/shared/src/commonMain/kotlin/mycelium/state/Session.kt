package mycelium.state

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import mycelium.api.Conversation
import mycelium.api.Event
import mycelium.api.EventKinds

data class Choice(val id: String, val name: String, val description: String? = null, val usedPercent: Int? = null, val group: String? = null)

data class Setting(val id: String, val name: String, val category: String?, val current: String?, val choices: List<Choice>)

data class SessionOptions(
    val modes: List<Choice>,
    val currentMode: String?,
    val models: List<Choice>,
    val currentModel: String?,
    val settings: List<Setting>,
)

data class SlashCommand(val name: String, val description: String, val hint: String?)

val SideChatCommands = listOf(
    SlashCommand("btw", "Ask without changing this conversation", null),
    SlashCommand("side", "Run a side task", null),
    SlashCommand("fork", "Copy into a new conversation", null),
)

private val sideChatCommand = Regex("""^/(btw|side|fork)(?:\s+(.*))?$""", RegexOption.DOT_MATCHES_ALL)

fun sideChatOf(message: String): Pair<String, String>? {
    val found = sideChatCommand.matchEntire(message.trim()) ?: return null

    return found.groupValues[1] to found.groupValues[2].trim()
}

fun sessionOptions(conversation: Conversation, events: List<Event>, kept: List<JsonObject> = emptyList(), reportedModels: List<Choice> = emptyList()): SessionOptions {
    val stored = conversation.options
    val modes = stored?.get("modes") as? JsonObject
    val models = stored?.get("models") as? JsonObject
    val configUpdate = latestUpdate(events, kept, "config_option_update")
    val settings = if (configUpdate != null) settingsFrom(configUpdate["configOptions"]) else settingsFrom(stored?.get("config_options"))
    val modeUpdate = latestUpdate(events, kept, "current_mode_update") { it.string("currentModeId") != null }
    val currentMode = modeUpdate?.string("currentModeId") ?: conversation.mode ?: modes?.string("currentModeId")

    return SessionOptions(
        modes = choicesFrom(modes?.get("availableModes"), "id"),
        currentMode = currentMode,
        models = choicesFrom(models?.get("availableModels"), "modelId").ifEmpty { reportedModels },
        currentModel = conversation.model ?: models?.string("currentModelId"),
        settings = settings,
    )
}

fun slashCommands(events: List<Event>, kept: List<JsonObject> = emptyList()): List<SlashCommand> {
    val latest = latestUpdate(events, kept, "available_commands_update") ?: return emptyList()
    val commands = latest["availableCommands"] as? JsonArray ?: return emptyList()

    return commands.mapNotNull { element ->
        val command = element as? JsonObject ?: return@mapNotNull null
        val name = command.string("name") ?: return@mapNotNull null
        val hint = (command["input"] as? JsonObject)?.string("hint")

        SlashCommand(name, command.string("description").orEmpty(), hint)
    }
}

private fun latestUpdate(events: List<Event>, kept: List<JsonObject>, kind: String, accept: (JsonObject) -> Boolean = { true }): JsonObject? {
    val fromEvents = events.asReversed().firstNotNullOfOrNull { event ->
        val update = if (event.kind == EventKinds.AcpUpdate) (event.payload as? JsonObject)?.get("update") as? JsonObject else null

        update?.takeIf { it.string("sessionUpdate") == kind && accept(it) }
    }

    return fromEvents ?: kept.lastOrNull { it.string("sessionUpdate") == kind && accept(it) }
}

private fun settingsFrom(element: JsonElement?): List<Setting> {
    val list = element as? JsonArray ?: return emptyList()

    return list.mapNotNull { item ->
        val setting = item as? JsonObject ?: return@mapNotNull null
        val id = setting.string("id") ?: return@mapNotNull null
        val choices = (setting["options"] as? JsonArray).orEmpty().flatMap { option ->
            val group = option as? JsonObject ?: return@flatMap emptyList()
            val grouped = group["options"] as? JsonArray

            if (grouped != null) choicesFrom(grouped, "value") else choicesFrom(JsonArray(listOf(group)), "value")
        }

        Setting(id, setting.string("name") ?: id, setting.string("category"), setting.string("currentValue"), choices)
    }
}

private fun choicesFrom(element: JsonElement?, idKey: String): List<Choice> {
    val list = element as? JsonArray ?: return emptyList()

    return list.mapNotNull { item ->
        val choice = item as? JsonObject ?: return@mapNotNull null
        val id = choice.string(idKey) ?: return@mapNotNull null

        Choice(id, choice.string("name")?.ifBlank { null } ?: id, choice.string("description"))
    }
}

data class Mention(val start: Int, val query: String)

fun mentionAt(text: String, cursor: Int): Mention? {
    val before = text.substring(0, cursor.coerceIn(0, text.length))
    val start = before.lastIndexOf('@')

    if (start < 0 || (start > 0 && !before[start - 1].isWhitespace())) {
        return null
    }

    val query = before.substring(start + 1)

    return if (query.any(Char::isWhitespace)) null else Mention(start, query)
}
