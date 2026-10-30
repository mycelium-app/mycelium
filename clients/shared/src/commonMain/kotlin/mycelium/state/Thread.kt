package mycelium.state

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.longOrNull
import mycelium.api.Attachment
import mycelium.api.Event
import mycelium.api.EventKinds
import mycelium.api.MyceliumJson
import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Instant

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

data class WorkflowAgent(
    val key: String,
    val label: String,
    val phase: String,
    val finished: Boolean,
    val tokens: Long? = null,
    val turns: Int? = null,
    val elapsedMs: Long? = null,
    val agentId: String? = null,
    val failed: Boolean = false,
    val paused: Boolean = false,
)

data class WorkflowEnd(val state: String, val endedAt: Instant? = null, val reason: String? = null)

val WorkflowStalledAfter = 15.minutes

data class WorkflowProgress(val runId: String, val agents: List<WorkflowAgent>, val plannedPhases: List<String> = emptyList(), val lastLineAt: Instant? = null, val limit: UsageLimitNote? = null, val end: WorkflowEnd? = null) {
    val ended: Boolean get() = end != null
    val stopped: Boolean get() = end?.state == WorkflowStates.Stopped
    val failed: Boolean get() = end?.state == WorkflowStates.Failed
    val over: Boolean get() = complete || ended
    val paused: Boolean get() = limit != null && !over
    val pausedUntil: String? get() = limit?.resetsAt
    val finishedCount: Int get() = agents.count { it.finished }
    val complete: Boolean get() = end?.state == WorkflowStates.Completed || (end == null && agents.isNotEmpty() && finishedCount == agents.size)
    val phases: List<String> get() = (plannedPhases + agents.map { it.phase }).distinct()
    val totals: WorkflowAgent get() = WorkflowAgent(
        key = runId,
        label = "Workflow",
        phase = "",
        finished = complete,
        tokens = agents.mapNotNull { it.tokens }.takeIf { it.isNotEmpty() }?.sum(),
        turns = agents.mapNotNull { it.turns }.takeIf { it.isNotEmpty() }?.sum(),
        elapsedMs = agents.mapNotNull { it.elapsedMs }.takeIf { it.isNotEmpty() }?.sum(),
    )
}

object WorkflowStates {
    const val Completed = "completed"
    const val Failed = "failed"
    const val Stopped = "stopped"
}

fun WorkflowProgress.quietFor(now: Instant): Duration? = lastLineAt?.let { now - it }

fun WorkflowProgress.stalled(now: Instant): Boolean = !over && !paused && (quietFor(now) ?: Duration.ZERO) >= WorkflowStalledAfter

fun workflowStats(agent: WorkflowAgent): String? = listOfNotNull(
    agent.tokens?.let { "${tokenCount(it)} toks" },
    agent.turns?.let { if (it == 1) "1 turn" else "$it turns" },
    agent.elapsedMs?.let { durationText(it) },
).takeIf { it.isNotEmpty() }?.joinToString(" · ")

fun workflowTokens(progress: WorkflowProgress): String? = progress.totals.tokens?.let { "${tokenCount(it)} toks" }

private fun tokenCount(tokens: Long): String = when {
    tokens >= 1_000_000 -> "${tokens / 100_000 / 10.0}M"
    tokens >= 1_000 -> "${tokens / 100 / 10.0}k"
    else -> tokens.toString()
}

private fun durationText(ms: Long): String {
    val seconds = ms / 1000
    val minutes = seconds / 60
    val hours = minutes / 60

    return when {
        hours > 0 -> "${hours}h ${(minutes % 60).toString().padStart(2, '0')}m"
        minutes > 0 -> "${minutes}m ${(seconds % 60).toString().padStart(2, '0')}s"
        else -> "${seconds}s"
    }
}

data class ThreadImage(val sha256: String, val name: String, val mediaType: String, val data: String? = null, val sameAs: String? = null)

sealed interface ThreadItem {
    val key: String

    data class UserMessage(
        override val key: String,
        val text: String,
        val files: List<Attachment> = emptyList(),
        val images: List<ThreadImage> = emptyList(),
        val goalSentAt: String? = null,
        val goalNudge: Int? = null,
        val goalWhy: String? = null,
        val spoken: Boolean = false,
        val spokenAt: String? = null,
    ) : ThreadItem

    data class GoalReport(override val key: String, val outcome: String, val reason: String, val at: String) : ThreadItem

    data class AgentMessage(override val key: String, val text: String) : ThreadItem

    data class Thought(override val key: String, val text: String, val finished: Boolean = false) : ThreadItem

    data class ToolCall(
        override val key: String,
        val callId: String,
        val title: String,
        val kind: String,
        val argument: String,
        val status: ToolStatus,
        val diff: FileDiff?,
        val output: String?,
        val images: List<ThreadImage> = emptyList(),
        val input: JsonObject? = null,
        val awaitingApproval: Boolean = false,
    ) : ThreadItem

    data class Plan(override val key: String, val entries: List<PlanEntry>) : ThreadItem

    data class Permission(
        override val key: String,
        val requestId: String,
        val title: String,
        val options: List<PermissionOption>,
        val resolved: Boolean,
        val server: String? = null,
        val account: String? = null,
        val filter: String? = null,
        val editsByDefault: Boolean = false,
        val input: JsonObject? = null,
        val callId: String? = null,
    ) : ThreadItem

    data class ResponseEdit(
        override val key: String,
        val requestId: String,
        val server: String,
        val account: String?,
        val tool: String,
        val resolved: Boolean = false,
        val outcome: String? = null,
    ) : ThreadItem

    data class Note(override val key: String, val text: String) : ThreadItem

    data class EnvironmentLine(override val key: String, val event: String, val text: String, val why: String, val at: String?) : ThreadItem

    data class TurnEnd(override val key: String, val stopReason: String) : ThreadItem

    data class Failure(override val key: String, val message: String, val limit: UsageLimitNote? = null) : ThreadItem

    data class Workflow(override val key: String, val progress: WorkflowProgress) : ThreadItem

    data class ModelChanged(override val key: String, val provider: String, val model: String) : ThreadItem

    data class Continued(override val key: String, val text: String, val from: ContinuedFrom? = null, val why: String? = null) : ThreadItem

    data class TeachMode(override val key: String, val on: Boolean) : ThreadItem
}

data class ContinuedFrom(val agent: String, val account: String?, val model: String?, val provider: String?)

data class UsageLimitNote(val window: String, val resetsAt: String?)

private fun usageLimitFrom(payload: JsonObject): UsageLimitNote? {
    val limit = payload["usage_limit"] as? JsonObject ?: return null

    return UsageLimitNote(limit.string("window") ?: "usage", limit.string("resets_at"))
}

private const val MaxDiffSide = 400

fun buildThread(events: List<Event>): List<ThreadItem> = ThreadBuilder().build(events)

fun turnIsOpen(events: List<Event>): Boolean = events.lastOrNull { it.kind == EventKinds.UserMessage || it.kind == EventKinds.TurnEnd }?.kind == EventKinds.UserMessage

class ThreadBuilder {
    private val items = mutableListOf<ThreadItem>()
    private var folded = 0
    private var firstFolded: Event? = null
    private var lastFolded: Event? = null
    private var built: List<ThreadItem> = emptyList()

    fun build(events: List<Event>): List<ThreadItem> {
        if (!extendsFolded(events)) {
            items.clear()
            folded = 0
            firstFolded = null
            lastFolded = null
            built = emptyList()
        }

        if (folded > 0 && folded == events.size) {
            return built
        }

        val fresh = if (folded == 0) events.sortedBy { it.seq } else events.subList(folded, events.size)

        for (event in fresh) {
            fold(event)
        }

        val inOrder = folded > 0 || fresh.indices.all { fresh[it] === events[it] }

        if (inOrder) {
            folded = events.size
            firstFolded = events.firstOrNull()
            lastFolded = events.lastOrNull()
        }

        val thoughtsEnded = items.mapIndexed { index, item -> if (item is ThreadItem.Thought && index < items.lastIndex) item.copy(finished = true) else item }

        built = withSameImagesMarked(thoughtsEnded)

        return built
    }

    private fun extendsFolded(events: List<Event>): Boolean {
        if (folded == 0 || events.size < folded || events[0] !== firstFolded || events[folded - 1] !== lastFolded) {
            return false
        }

        var previous = lastFolded?.seq ?: return false

        for (index in folded until events.size) {
            val seq = events[index].seq

            if (seq <= previous) {
                return false
            }

            previous = seq
        }

        return true
    }

    private fun addFailure(key: String, payload: JsonObject) {
        val limit = usageLimitFrom(payload)

        items.removeAll { it is ThreadItem.Failure && limit != null && it.limit == limit }
        items += ThreadItem.Failure(key, payload.string("message") ?: "The agent stopped with an error.", limit)
    }

    private fun fold(event: Event) {
        val payload = event.payload as? JsonObject ?: JsonObject(emptyMap())
        val key = "e${event.seq}"

        when (event.kind) {
            EventKinds.UserMessage -> items += userMessageFrom(key, payload)
            EventKinds.AcpUpdate -> applyUpdate(items, key, payload)
            EventKinds.PermissionRequest -> requestPermission(items, permissionFrom(key, payload))
            EventKinds.PermissionResolved -> resolvePermission(items, payload.string("request_id"))
            EventKinds.ReviewRequest -> items += ThreadItem.ResponseEdit(key, payload.string("request_id") ?: key, payload.string("server").orEmpty(), payload.string("account"), payload.string("tool").orEmpty())
            EventKinds.ReviewResolved -> resolveResponseEdit(items, payload)
            EventKinds.TurnEnd -> items += ThreadItem.TurnEnd(key, payload.string("stop_reason") ?: "end_turn")
            EventKinds.Error -> addFailure(key, payload)
            EventKinds.Workflow -> applyWorkflowLine(items, key, payload)
            EventKinds.ModelChanged -> items += ThreadItem.ModelChanged(key, payload.string("provider_name").orEmpty(), payload.string("model").orEmpty())
            EventKinds.Continued -> items += continuedItem(key, payload)
            EventKinds.TeachMode -> items += ThreadItem.TeachMode(key, payload.boolean("on") ?: false)
            EventKinds.Environment -> items += environmentItem(key, payload)
            EventKinds.GoalReport -> items += ThreadItem.GoalReport(key, payload.string("outcome").orEmpty(), payload.string("reason").orEmpty(), payload.string("at").orEmpty())
        }
    }
}

private fun userMessageFrom(key: String, payload: JsonObject): ThreadItem.UserMessage {
    val attachments = (payload["attachments"] as? JsonArray).orEmpty().mapNotNull { element ->
        val entry = element as? JsonObject ?: return@mapNotNull null

        Attachment(
            sha256 = entry.string("sha256") ?: return@mapNotNull null,
            name = entry.string("name") ?: "attachment",
            mediaType = entry.string("media_type") ?: "application/octet-stream",
            size = entry.string("size")?.toLongOrNull() ?: 0,
        )
    }
    val (pictures, files) = attachments.partition { isImage(it.mediaType) }

    val goalSentAt = payload.string("sent_at").takeIf { payload.string("origin") == "goal" }

    val fromGoal = payload.string("origin") == "goal"

    return ThreadItem.UserMessage(key, payload.string("text") ?: "", files, pictures.map { ThreadImage(it.sha256, it.name, it.mediaType) }, goalSentAt, payload.int("nudge")?.takeIf { fromGoal && it > 0 }, payload.string("why").takeIf { fromGoal }, payload.boolean("spoken") == true, payload.string("spoken_at"))
}

private fun withSameImagesMarked(items: List<ThreadItem>): List<ThreadItem> {
    val firstSeen = mutableMapOf<String, String>()

    fun marked(key: String, images: List<ThreadImage>): List<ThreadImage> = images.map { image ->
        val earlier = firstSeen[image.sha256]

        if (earlier == null) {
            firstSeen[image.sha256] = key
            image
        } else {
            image.copy(sameAs = earlier)
        }
    }

    return items.map { item ->
        when {
            item is ThreadItem.UserMessage && item.images.isNotEmpty() -> item.copy(images = marked(item.key, item.images))
            item is ThreadItem.ToolCall && item.images.isNotEmpty() -> item.copy(images = marked(item.key, item.images))
            else -> item
        }
    }
}

private const val MaxSuggestions = 4

private const val MaxSuggestionLength = 80

private val listItem = Regex("""^(?:\d+[.)]|[-*\u2022]|[A-Da-d][.)])\s+(.+)$""")

private val offersChoice = Regex("""\b(?:options?|either|choose|pick|prefer|which|alternatives?|approaches?|routes?)\b""", RegexOption.IGNORE_CASE)

private val yesNoQuestion = Regex("""^(?:want me to|would you like|should i|shall i|do you want|can i|may i)\b""", RegexOption.IGNORE_CASE)

fun suggestedReplies(answer: String): List<String> {
    val lines = answer.lines().map { it.trim() }.filter { it.isNotEmpty() }
    val closing = lines.lastOrNull().orEmpty()
    val asksClosing = !listItem.matches(closing) && closing.endsWith("?")
    val body = if (asksClosing) lines.dropLast(1) else lines
    val items = body.takeLastWhile { listItem.matches(it) }
    val lead = body.dropLast(items.size).lastOrNull().orEmpty()
    val asksWhich = asksClosing || lead.endsWith("?") || (lead.endsWith(":") && offersChoice.containsMatchIn(lead))

    if (items.size in 2..MaxSuggestions && asksWhich) {
        val replies = items.mapNotNull { line -> listItem.matchEntire(line)?.groupValues?.get(1)?.replace(Regex("""[*_`]"""), "")?.trim()?.trimEnd('.', ';', ',') }

        if (replies.all { it.isNotEmpty() && it.length <= MaxSuggestionLength }) {
            return replies
        }
    }

    if (asksClosing && yesNoQuestion.containsMatchIn(closing.substringAfterLast(". "))) {
        return listOf("Yes", "No")
    }

    return emptyList()
}

private fun applyUpdate(items: MutableList<ThreadItem>, key: String, payload: JsonObject) {
    val update = payload["update"] as? JsonObject ?: return

    when (update.string("sessionUpdate")) {
        "agent_message_chunk" -> appendAgentText(items, key, update.contentText())
        "agent_thought_chunk" -> appendThought(items, key, update.contentText())
        "user_message_chunk" -> Unit
        "tool_call" -> {
            val call = toolCallFrom(key, update)
            items += call.copy(awaitingApproval = items.any { it is ThreadItem.Permission && !it.resolved && it.callId == call.callId })
        }
        "tool_call_update" -> mergeToolUpdate(items, update)
        "note" -> items += ThreadItem.Note(key, update.contentText())
        "plan" -> items += ThreadItem.Plan(key, planEntries(update))
    }
}

private fun appendThought(items: MutableList<ThreadItem>, key: String, text: String) {
    val last = items.lastOrNull()

    if (last is ThreadItem.Thought) {
        items[items.lastIndex] = last.copy(text = last.text + text)
        return
    }

    items += ThreadItem.Thought(key, text)
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
    val title = update.string("name")?.takeIf { it == "functions.exec" } ?: update.string("title") ?: kind

    return ThreadItem.ToolCall(
        key = key,
        callId = update.string("toolCallId") ?: key,
        title = title,
        kind = kind,
        argument = toolArgument(update, title),
        status = toolStatus(update.string("status")),
        diff = diffFrom(update),
        output = outputFrom(update),
        images = imagesFrom(update, title),
        input = inputFrom(update),
    )
}

private fun mergeToolUpdate(items: MutableList<ThreadItem>, update: JsonObject) {
    val callId = update.string("toolCallId") ?: return
    val index = items.indexOfLast { it is ThreadItem.ToolCall && it.callId == callId }

    if (index < 0) {
        return
    }

    val current = items[index] as ThreadItem.ToolCall
    val title = update.string("name")?.takeIf { it == "functions.exec" } ?: update.string("title") ?: current.title
    val argument = toolArgument(update, title)
    val namesInput = argument != title || current.argument == current.title

    items[index] = current.copy(
        title = title,
        argument = if (namesInput) argument else current.argument,
        status = update.string("status")?.let(::toolStatus) ?: current.status,
        diff = diffFrom(update) ?: current.diff,
        output = outputFrom(update) ?: current.output,
        images = imagesFrom(update, title).ifEmpty { current.images },
        input = inputFrom(update) ?: current.input,
    )
}

private fun inputFrom(update: JsonObject): JsonObject? {
    val input = update["rawInput"] as? JsonObject ?: return null
    val batch = update.string("name") == "functions.exec" || update.string("title") == "functions.exec"
    val wrapped = batch || input.string("server") != null && input.string("tool") != null
    val arguments = input["arguments"]

    if (wrapped && arguments is JsonObject) {
        return arguments
    }

    if (wrapped && arguments is JsonPrimitive && arguments.isString) {
        val parsed = runCatching { MyceliumJson.parseToJsonElement(arguments.content) }.getOrNull()

        if (parsed is JsonObject) {
            return parsed
        }

        if (batch) {
            return JsonObject(mapOf("code" to arguments))
        }
    }

    return input.takeIf { it.isNotEmpty() }
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

private const val MaxHashedImages = 64

private val imageHashes = mutableMapOf<Pair<Int, Int>, String>()

private fun imagesFrom(update: JsonObject, title: String): List<ThreadImage> =
    toolContent(update).filter { it.string("type") == "content" }.mapNotNull { block ->
        val image = block["content"] as? JsonObject ?: return@mapNotNull null
        val data = image.string("data")

        if (image.string("type") != "image" || data == null) {
            return@mapNotNull null
        }

        val cacheKey = data.hashCode() to data.length
        val hash = imageHashes[cacheKey] ?: decodeBase64(data)?.let(::sha256Hex)?.also {
            if (imageHashes.size >= MaxHashedImages) {
                imageHashes.clear()
            }

            imageHashes[cacheKey] = it
        } ?: return@mapNotNull null

        ThreadImage(hash, title, image.string("mimeType") ?: "image/png", data)
    }

private fun outputFrom(update: JsonObject): String? {
    val texts = toolContent(update)
        .filter { it.string("type") == "content" }
        .mapNotNull { (it["content"] as? JsonObject)?.string("text") }

    if (texts.isNotEmpty()) {
        return texts.joinToString("\n") { unfenced(it) }
    }

    val raw = update["rawOutput"] as? JsonObject ?: return null
    val result = raw["result"] as? JsonObject
    val error = (raw["error"] as? JsonObject)?.string("message")
    val resultTexts = (result?.get("content") as? JsonArray).orEmpty().mapNotNull {
        val content = it as? JsonObject ?: return@mapNotNull null

        content.string("text")?.takeIf { content.string("type") == "text" }
    }
    val text = resultTexts.takeIf { it.isNotEmpty() }?.joinToString("\n")
    val structured = result?.get("structuredContent")?.takeUnless { it == JsonNull }?.toString()

    return listOfNotNull(text ?: structured, error).takeIf { it.isNotEmpty() }?.joinToString("\n")
}

private fun unfenced(text: String): String {
    val fence = Regex("^```[\\w-]*\n([\\s\\S]*?)\n?```\\s*$").find(text.trim()) ?: return text

    return fence.groupValues[1]
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
        server = toolCall?.string("server"),
        account = toolCall?.string("account"),
        filter = toolCall?.string("filter"),
        editsByDefault = (toolCall?.get("edit") as? JsonPrimitive)?.booleanOrNull == true,
        input = toolCall?.get("rawInput") as? JsonObject,
        callId = toolCall?.string("toolCallId"),
    )
}

private fun requestPermission(items: MutableList<ThreadItem>, permission: ThreadItem.Permission) {
    items += permission
    setAwaitingApproval(items, permission.callId, true)
}

private fun setAwaitingApproval(items: MutableList<ThreadItem>, callId: String?, awaiting: Boolean) {
    val index = items.indexOfLast { it is ThreadItem.ToolCall && it.callId == callId }

    if (callId != null && index >= 0) {
        items[index] = (items[index] as ThreadItem.ToolCall).copy(awaitingApproval = awaiting)
    }
}

private fun resolvePermission(items: MutableList<ThreadItem>, requestId: String?) {
    val index = items.indexOfLast { it is ThreadItem.Permission && it.requestId == requestId }

    if (index >= 0) {
        val permission = (items[index] as ThreadItem.Permission).copy(resolved = true)
        items[index] = permission
        setAwaitingApproval(items, permission.callId, false)
    }
}

private fun resolveResponseEdit(items: MutableList<ThreadItem>, payload: JsonObject) {
    val index = items.indexOfLast { it is ThreadItem.ResponseEdit && it.requestId == payload.string("request_id") }

    if (index >= 0) {
        items[index] = (items[index] as ThreadItem.ResponseEdit).copy(resolved = true, outcome = payload.string("outcome") ?: legacyOutcome(payload.string("decision")))
    }
}

private fun legacyOutcome(decision: String?): String = when (decision) {
    "share" -> "sent"
    "remove" -> "edited"
    else -> "nothing"
}

private fun applyWorkflowLine(items: MutableList<ThreadItem>, key: String, payload: JsonObject) {
    val runId = payload.string("run_id") ?: return
    val line = payload["line"] as? JsonObject ?: return
    val index = items.indexOfFirst { it is ThreadItem.Workflow && it.progress.runId == runId }
    val current = (items.getOrNull(index) as? ThreadItem.Workflow)?.progress ?: WorkflowProgress(runId, emptyList())
    val planned = (line["phases"] as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }.takeIf { line.string("type") == "plan" }
    val ended = workflowEnd(line)
    val limit = usageLimitFrom(payload)
    val journaled = applyWorkflowJournal(current.agents, line, paused = limit != null)
    val resumed = line.string("type") == "started"
    val applied = when {
        planned != null -> current.copy(plannedPhases = planned)
        ended != null -> current.copy(end = ended)
        limit != null -> current.copy(agents = journaled, limit = limit)
        resumed -> current.copy(agents = journaled, limit = null)
        else -> current.copy(agents = journaled)
    }
    val updated = applied.copy(lastLineAt = payload.string("at")?.let { instantOrNull(it) } ?: applied.lastLineAt)

    if (index < 0) {
        items += ThreadItem.Workflow(key, updated)
        return
    }

    items[index] = (items[index] as ThreadItem.Workflow).copy(progress = updated)
}

private fun workflowEnd(line: JsonObject): WorkflowEnd? {
    if (line.string("type") != "ended") {
        return null
    }

    val state = line.string("state") ?: return null

    return WorkflowEnd(state, line.string("ended_at")?.let { instantOrNull(it) }, line.string("reason"))
}

fun applyWorkflowJournal(agents: List<WorkflowAgent>, line: JsonObject, paused: Boolean = false): List<WorkflowAgent> {
    val agentKey = line.string("key") ?: return agents

    return when (line.string("type")) {
        "started" -> {
            if (agents.any { it.key == agentKey }) {
                agents.map { if (it.key == agentKey) it.copy(paused = false).withStats(line) else it }
            } else {
                agents + WorkflowAgent(
                    key = agentKey,
                    label = line.string("label") ?: "Agent",
                    phase = line.string("phase") ?: "Run",
                    finished = false,
                    agentId = line.string("agentId"),
                ).withStats(line)
            }
        }
        "result", "failed" -> if (paused) {
            agents.map { if (it.key == agentKey) it.copy(paused = true).withStats(line) else it }
        } else if (line.string("type") == "result") {
            agents.map { if (it.key == agentKey) it.copy(finished = true, paused = false).withStats(line) else it }
        } else {
            agents.map { if (it.key == agentKey) it.copy(finished = true, failed = true, paused = false).withStats(line) else it }
        }
        else -> agents.map { if (it.key == agentKey) it.withStats(line) else it }
    }
}

private fun WorkflowAgent.withStats(line: JsonObject) = copy(
    tokens = line.number("tokens") ?: tokens,
    turns = line.number("turns")?.toInt() ?: turns,
    elapsedMs = line.number("elapsed_ms") ?: elapsedMs,
)

private fun JsonObject.number(key: String): Long? = (this[key] as? JsonPrimitive)?.takeIf { !it.isString }?.longOrNull

fun diffLines(oldText: String, newText: String): List<DiffLine> {
    val allOld = if (oldText.isEmpty()) emptyList() else oldText.split("\n")
    val allNew = if (newText.isEmpty()) emptyList() else newText.split("\n")
    val shared = minOf(allOld.size, allNew.size)
    val head = (0 until shared).firstOrNull { allOld[it] != allNew[it] } ?: shared
    val tail = (0 until shared - head).firstOrNull { allOld[allOld.size - 1 - it] != allNew[allNew.size - 1 - it] } ?: (shared - head)
    val old = allOld.subList(head, allOld.size - tail)
    val new = allNew.subList(head, allNew.size - tail)
    val before = (0 until head).map { DiffLine(DiffLine.Kind.Context, it + 1, it + 1, allOld[it]) }
    val after = (0 until tail).map { offset -> DiffLine(DiffLine.Kind.Context, allOld.size - tail + offset + 1, allNew.size - tail + offset + 1, allOld[allOld.size - tail + offset]) }

    if (old.size > MaxDiffSide || new.size > MaxDiffSide) {
        return before + old.mapIndexed { i, text -> DiffLine(DiffLine.Kind.Removed, head + i + 1, null, text) } +
            new.mapIndexed { i, text -> DiffLine(DiffLine.Kind.Added, null, head + i + 1, text) } + after
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
                result += DiffLine(DiffLine.Kind.Context, head + i + 1, head + j + 1, old[i])
                i++
                j++
            }
            j < new.size && (i == old.size || lengths[i][j + 1] > lengths[i + 1][j]) -> {
                result += DiffLine(DiffLine.Kind.Added, null, head + j + 1, new[j])
                j++
            }
            else -> {
                result += DiffLine(DiffLine.Kind.Removed, head + i + 1, null, old[i])
                i++
            }
        }
    }

    return before + result + after
}

private fun sideLabel(agent: String, account: String?, model: String?, provider: String?): String =
    when {
        agent == "chat" -> listOfNotNull(provider, model).joinToString(" · ")
        else -> listOfNotNull(if (account != null) "${agentName(agent)} as ${account.substringBefore("@")}" else agentName(agent), model).joinToString(", ")
    }

private fun limitName(window: String?): String = when (window) {
    "five_hour" -> "5-hour limit"
    "seven_day" -> "weekly limit"
    else -> "usage limit"
}

private fun switchReason(reason: String, payload: JsonObject, from: ContinuedFrom?): String? = when (reason) {
    "usage_limit" -> if (payload.boolean("every_account") == true && from != null) "${limitName(payload.string("window"))} reached on every ${agentName(from.agent)} account" else "${limitName(payload.string("window"))} reached"
    "usage_returned" -> "${agentName(payload.string("agent").orEmpty())} has usage again"
    "signed_out" -> "signed out"
    "offline" -> "node offline"
    "model_error" -> "the model failed"
    else -> null
}

private fun environmentItem(key: String, payload: JsonObject): ThreadItem {
    val text = payload.string("text").orEmpty()
    val event = payload.string("event") ?: return ThreadItem.Note(key, text)

    return ThreadItem.EnvironmentLine(key, event, text, payload.string("why").orEmpty(), payload.string("at"))
}

private fun continuedItem(key: String, payload: JsonObject): ThreadItem.Continued {
    val agent = payload.string("agent").orEmpty()
    val target = sideLabel(agent, payload.string("account"), payload.string("model"), payload.string("provider_name"))
    val from = (payload["from"] as? JsonObject)?.let { ContinuedFrom(it.string("agent").orEmpty(), it.string("account"), it.string("model"), it.string("provider_name")) }
    val node = payload.string("node_name")
    val reason = payload.string("reason")?.let { switchReason(it, payload, from) }

    return when {
        payload.string("kind") == "any_node" -> ThreadItem.Continued(key, "Picked $node")
        payload.string("kind") == "scratch" -> ThreadItem.Continued(key, "Started in a scratch folder on $node. It is deleted with the chat.")
        reason != null -> ThreadItem.Continued(key, "Continued with ${node?.let { "$target on $it" } ?: target}", from, reason.replaceFirstChar { it.uppercase() })
        node != null -> ThreadItem.Continued(key, "Moved to $node", from)
        else -> ThreadItem.Continued(key, "Continued on $target", from)
    }
}

internal fun JsonObject.string(key: String): String? = (this[key] as? JsonPrimitive)?.contentOrNull

private fun JsonObject.int(key: String): Int? = string(key)?.toIntOrNull()

private fun JsonObject.boolean(key: String): Boolean? = string(key)?.toBooleanStrictOrNull()

private fun JsonObject.contentText(): String = (this["content"] as? JsonObject)?.string("text") ?: ""
