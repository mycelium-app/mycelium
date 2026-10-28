package mycelium.state

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

const val ToolSearchName = "tool_search"

private const val MaxArgumentLength = 240

private val chatToolTitle = Regex("^mcp__(.+?)__(.+)$")
private val codexToolTitle = Regex("^mcp\\.([^.]+)\\.(.+)$")

private const val EnvironmentToolPrefix = "environment_"

private val environmentToolHeadings = mapOf("launch" to "Open app")

data class ChatToolName(val server: String, val tool: String) {
    private val environmentAction: String? get() = tool.removePrefix(EnvironmentToolPrefix).takeIf { server == "mycelium" && tool.startsWith(EnvironmentToolPrefix) }

    val heading: String get() = when {
        server == "functions" && tool == "exec" -> "Run tools"
        environmentAction != null -> environmentToolHeadings[environmentAction] ?: environmentAction!!.replaceFirstChar { it.uppercase() }
        else -> tool.replace('_', ' ').replaceFirstChar { it.uppercase() }
    }

    val source: String get() = if (environmentAction != null) "environment" else server
}

fun chatToolName(title: String): ChatToolName? {
    if (title == "functions.exec") {
        return ChatToolName("functions", "exec")
    }

    val parts = chatToolTitle.matchEntire(title)?.groupValues ?: codexToolTitle.matchEntire(title)?.groupValues ?: return null

    return ChatToolName(parts[1], parts[2])
}

fun readableArguments(input: JsonObject?): List<Pair<String, String>> =
    input.orEmpty().map { (key, value) ->
        val text = if (value is JsonPrimitive && value.isString) value.content else value.toString()

        key to if (text.length > MaxArgumentLength) text.take(MaxArgumentLength) + "…" else text
    }
