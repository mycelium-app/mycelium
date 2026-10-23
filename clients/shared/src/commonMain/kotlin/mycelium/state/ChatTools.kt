package mycelium.state

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

const val ToolSearchName = "tool_search"

private const val MaxArgumentLength = 240

private val chatToolTitle = Regex("^mcp__(.+?)__(.+)$")

data class ChatToolName(val server: String, val tool: String) {
    val heading: String get() = tool.replace('_', ' ').replaceFirstChar { it.uppercase() }
}

fun chatToolName(title: String): ChatToolName? {
    val parts = chatToolTitle.matchEntire(title)?.groupValues ?: return null

    return ChatToolName(parts[1], parts[2])
}

fun readableArguments(input: JsonObject?): List<Pair<String, String>> =
    input.orEmpty().map { (key, value) ->
        val text = if (value is JsonPrimitive && value.isString) value.content else value.toString()

        key to if (text.length > MaxArgumentLength) text.take(MaxArgumentLength) + "…" else text
    }
