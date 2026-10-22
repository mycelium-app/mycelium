package mycelium.state

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import mycelium.api.Event
import mycelium.api.EventKinds

enum class ToolStatus { Pending, Running, Done, Failed }

enum class PlanStatus { Pending, Running, Done }

data class DiffLine(val kind: Kind, val oldNumber: Int?, val newNumber: Int?, val text: String) {
    enum class Kind { Context, Added, Removed }
}

data class FileDiff(val path: String, val lines: List<DiffLine>) {
    val added: Int get() = lines.count { it.kind == DiffLine.Kind.Added }
    val removed: Int get() = lines.count { it.kind == DiffLine.Kind.Removed }
}

data class PlanEntry(val content: String, val status: PlanStatus)

data class PermissionOption(val id: String, val name: String, val kind: String)

data class WorkflowAgent(val key: String, val label: String, val phase: String, val finished: Boolean)

data class WorkflowProgress(val runId: String, val agents: List<WorkflowAgent>) {
    val finishedCount: Int get() = agents.count { it.finished }
    val complete: Boolean get() = agents.isNotEmpty() && finishedCount == agents.size
    val phases: List<String> get() = agents.map { it.phase }.distinct()
}

sealed interface ThreadItem {
    val key: String

    data class UserMessage(override val key: String, val text: String) : ThreadItem

    data class AgentMessage(override val key: String, val text: String) : ThreadItem

    data class ToolCall(
        override val key: String,
        val callId: String,
        val title: String,
        val kind: String,
        val argument: String,
        val status: ToolStatus,
        val diff: FileDiff?,
        val output: String?,
    ) : ThreadItem

    data class Plan(override val key: String, val entries: List<PlanEntry>) : ThreadItem

    data class Permission(
        override val key: String,
        val requestId: String,
        val title: String,
        val options: List<PermissionOption>,
        val resolved: Boolean,
    ) : ThreadItem

    data class TurnEnd(override val key: String, val stopReason: String) : ThreadItem

    data class Failure(override val key: String, val message: String) : ThreadItem

    data class Workflow(override val key: String, val progress: WorkflowProgress) : ThreadItem
}

private const val MaxDiffSide = 400

fun buildThread(events: List<Event>): List<ThreadItem> {
    val items = mutableListOf<ThreadItem>()

    for (event in events.sortedBy { it.seq }) {
        val payload = event.payload as? JsonObject ?: JsonObject(emptyMap())
        val key = "e${event.seq}"

        when (event.kind) {
            EventKinds.UserMessage -> items += ThreadItem.UserMessage(key, payload.string("text") ?: "")
            EventKinds.AcpUpdate -> applyUpdate(items, key, payload)
            EventKinds.PermissionRequest -> items += permissionFrom(key, payload)
            EventKinds.PermissionResolved -> resolvePermission(items, payload.string("request_id"))
            EventKinds.TurnEnd -> items += ThreadItem.TurnEnd(key, payload.string("stop_reason") ?: "end_turn")
            EventKinds.Error -> items += ThreadItem.Failure(key, payload.string("message") ?: "The agent stopped with an error.")
            EventKinds.Workflow -> applyWorkflowLine(items, key, payload)
        }
    }

    return items
}

private fun applyUpdate(items: MutableList<ThreadItem>, key: String, payload: JsonObject) {
    val update = payload["update"] as? JsonObject ?: return

    when (update.string("sessionUpdate")) {
        "agent_message_chunk" -> appendAgentText(items, key, update.contentText())
        "user_message_chunk" -> Unit
        "tool_call" -> items += toolCallFrom(key, update)
        "tool_call_update" -> mergeToolUpdate(items, update)
        "plan" -> items += ThreadItem.Plan(key, planEntries(update))
    }
}

private fun appendAgentText(items: MutableList<ThreadItem>, key: String, text: String) {
    val last = items.lastOrNull()

    if (last is ThreadItem.AgentMessage) {
        items[items.lastIndex] = last.copy(text = last.text + text)
        return
    }

    items += ThreadItem.AgentMessage(key, text)
}

private fun toolCallFrom(key: String, update: JsonObject): ThreadItem.ToolCall {
    val kind = update.string("kind") ?: "other"
    val title = update.string("title") ?: kind

    return ThreadItem.ToolCall(
        key = key,
        callId = update.string("toolCallId") ?: key,
        title = title,
        kind = kind,
        argument = toolArgument(update, title),
        status = toolStatus(update.string("status")),
        diff = diffFrom(update),
        output = outputFrom(update),
    )
}

private fun mergeToolUpdate(items: MutableList<ThreadItem>, update: JsonObject) {
    val callId = update.string("toolCallId") ?: return
    val index = items.indexOfLast { it is ThreadItem.ToolCall && it.callId == callId }

    if (index < 0) {
        return
    }

    val current = items[index] as ThreadItem.ToolCall

    items[index] = current.copy(
        title = update.string("title") ?: current.title,
        status = update.string("status")?.let(::toolStatus) ?: current.status,
        diff = diffFrom(update) ?: current.diff,
        output = outputFrom(update) ?: current.output,
    )
}

private fun toolArgument(update: JsonObject, title: String): String {
    val locations = update["locations"] as? JsonArray
    val path = locations?.firstNotNullOfOrNull { (it as? JsonObject)?.string("path") }

    if (path != null) {
        return path
    }

    val rawInput = update["rawInput"] as? JsonObject
    val command = rawInput?.string("command") ?: rawInput?.string("file_path") ?: rawInput?.string("path")

    return command ?: title
}

private fun toolStatus(value: String?): ToolStatus = when (value) {
    "in_progress" -> ToolStatus.Running
    "completed" -> ToolStatus.Done
    "failed" -> ToolStatus.Failed
    else -> ToolStatus.Pending
}

private fun toolContent(update: JsonObject): List<JsonObject> =
    (update["content"] as? JsonArray)?.mapNotNull { it as? JsonObject } ?: emptyList()

private fun diffFrom(update: JsonObject): FileDiff? {
    val block = toolContent(update).firstOrNull { it.string("type") == "diff" } ?: return null
    val path = block.string("path") ?: return null

    return FileDiff(path, diffLines(block.string("oldText") ?: "", block.string("newText") ?: ""))
}

private fun outputFrom(update: JsonObject): String? {
    val texts = toolContent(update)
        .filter { it.string("type") == "content" }
        .mapNotNull { (it["content"] as? JsonObject)?.string("text") }

    return texts.takeIf { it.isNotEmpty() }?.joinToString("\n")
}

private fun planEntries(update: JsonObject): List<PlanEntry> =
    (update["entries"] as? JsonArray).orEmpty().mapNotNull { element ->
        val entry = element as? JsonObject ?: return@mapNotNull null

        PlanEntry(
            content = entry.string("content") ?: return@mapNotNull null,
            status = when (entry.string("status")) {
                "completed" -> PlanStatus.Done
                "in_progress" -> PlanStatus.Running
                else -> PlanStatus.Pending
            },
        )
    }

private fun permissionFrom(key: String, payload: JsonObject): ThreadItem.Permission {
    val toolCall = payload["toolCall"] as? JsonObject
    val options = (payload["options"] as? JsonArray).orEmpty().mapNotNull { element ->
        val option = element as? JsonObject ?: return@mapNotNull null

        PermissionOption(
            id = option.string("optionId") ?: return@mapNotNull null,
            name = option.string("name") ?: "Choose",
            kind = option.string("kind") ?: "allow_once",
        )
    }

    return ThreadItem.Permission(
        key = key,
        requestId = payload.string("request_id") ?: key,
        title = toolCall?.string("title") ?: "Permission needed",
        options = options,
        resolved = false,
    )
}

private fun resolvePermission(items: MutableList<ThreadItem>, requestId: String?) {
    val index = items.indexOfLast { it is ThreadItem.Permission && it.requestId == requestId }

    if (index >= 0) {
        items[index] = (items[index] as ThreadItem.Permission).copy(resolved = true)
    }
}

private fun applyWorkflowLine(items: MutableList<ThreadItem>, key: String, payload: JsonObject) {
    val runId = payload.string("run_id") ?: return
    val line = payload["line"] as? JsonObject ?: return
    val index = items.indexOfFirst { it is ThreadItem.Workflow && it.progress.runId == runId }
    val current = (items.getOrNull(index) as? ThreadItem.Workflow)?.progress ?: WorkflowProgress(runId, emptyList())
    val updated = current.copy(agents = applyWorkflowJournal(current.agents, line))

    if (index < 0) {
        items += ThreadItem.Workflow(key, updated)
        return
    }

    items[index] = (items[index] as ThreadItem.Workflow).copy(progress = updated)
}

fun applyWorkflowJournal(agents: List<WorkflowAgent>, line: JsonObject): List<WorkflowAgent> {
    val agentKey = line.string("key") ?: return agents

    return when (line.string("type")) {
        "started" -> {
            if (agents.any { it.key == agentKey }) {
                agents
            } else {
                agents + WorkflowAgent(
                    key = agentKey,
                    label = line.string("label") ?: "Agent",
                    phase = line.string("phase") ?: "Run",
                    finished = false,
                )
            }
        }
        "result" -> agents.map { if (it.key == agentKey) it.copy(finished = true) else it }
        else -> agents
    }
}

fun diffLines(oldText: String, newText: String): List<DiffLine> {
    val old = if (oldText.isEmpty()) emptyList() else oldText.split("\n")
    val new = if (newText.isEmpty()) emptyList() else newText.split("\n")

    if (old.size > MaxDiffSide || new.size > MaxDiffSide) {
        return old.mapIndexed { i, text -> DiffLine(DiffLine.Kind.Removed, i + 1, null, text) } +
            new.mapIndexed { i, text -> DiffLine(DiffLine.Kind.Added, null, i + 1, text) }
    }

    val lengths = Array(old.size + 1) { IntArray(new.size + 1) }

    for (i in old.indices.reversed()) {
        for (j in new.indices.reversed()) {
            lengths[i][j] = if (old[i] == new[j]) lengths[i + 1][j + 1] + 1 else maxOf(lengths[i + 1][j], lengths[i][j + 1])
        }
    }

    val result = mutableListOf<DiffLine>()
    var i = 0
    var j = 0

    while (i < old.size || j < new.size) {
        when {
            i < old.size && j < new.size && old[i] == new[j] -> {
                result += DiffLine(DiffLine.Kind.Context, i + 1, j + 1, old[i])
                i++
                j++
            }
            j < new.size && (i == old.size || lengths[i][j + 1] > lengths[i + 1][j]) -> {
                result += DiffLine(DiffLine.Kind.Added, null, j + 1, new[j])
                j++
            }
            else -> {
                result += DiffLine(DiffLine.Kind.Removed, i + 1, null, old[i])
                i++
            }
        }
    }

    return result
}

private fun JsonObject.string(key: String): String? = (this[key] as? JsonPrimitive)?.contentOrNull

private fun JsonObject.contentText(): String = (this["content"] as? JsonObject)?.string("text") ?: ""
