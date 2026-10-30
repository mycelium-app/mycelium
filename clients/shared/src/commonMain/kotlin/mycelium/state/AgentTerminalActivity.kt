package mycelium.state

import kotlinx.serialization.json.JsonObject
import mycelium.api.Event
import mycelium.api.EventKinds

enum class AgentTerminalActivity { Reading, Typing }

private const val RecentEvents = 200

fun agentTerminalActivity(events: List<Event>, working: Boolean): Map<String, AgentTerminalActivity> {
    if (!working) {
        return emptyMap()
    }

    val latest = mutableMapOf<String, AgentTerminalActivity>()

    for (event in events.takeLast(RecentEvents)) {
        val update = (event.payload as? JsonObject)?.get("update") as? JsonObject ?: continue

        if (event.kind != EventKinds.AcpUpdate || update.string("sessionUpdate") != "tool_call") {
            continue
        }

        val title = update.string("title").orEmpty()
        val terminalId = (update["rawInput"] as? JsonObject)?.string("terminal_id") ?: continue

        when {
            title.endsWith("terminal_type") -> latest[terminalId] = AgentTerminalActivity.Typing
            title.endsWith("terminal_read") -> latest[terminalId] = AgentTerminalActivity.Reading
        }
    }

    return latest
}
