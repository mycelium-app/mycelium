package mycelium.state

import kotlinx.serialization.json.JsonPrimitive

fun toolSearchText(call: ThreadItem.ToolCall): String? {
    if (!call.title.equals("ToolSearch", ignoreCase = true)) {
        return null
    }

    val query = (call.input?.get("query") as? JsonPrimitive)?.content?.trim()

    if (query.isNullOrEmpty()) {
        return if (call.status == ToolStatus.Running) "Loading tools" else "Loaded tools"
    }

    if (!query.startsWith("select:")) {
        return "Searched tools for “$query”"
    }

    val names = query.removePrefix("select:").split(',').map { it.trim() }.filter { it.isNotEmpty() }.map(::readableToolName)
    val verb = if (call.status == ToolStatus.Running) "Loading" else "Loaded"

    return "$verb ${names.joinToString(", ")}"
}

private fun readableToolName(name: String): String {
    val parts = name.split("__")

    if (parts.size < 3 || parts.first() != "mcp") {
        return name
    }

    val tool = parts.drop(2).joinToString("__")
    val server = parts[1].replace('_', ' ')
    val label = myceliumTool("mycelium $tool")?.takeIf { server == "mycelium" }?.label ?: tool.replace('_', ' ')

    return "$label from ${server.replaceFirstChar { it.uppercase() }}"
}
