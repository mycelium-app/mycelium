package mycelium.state

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

enum class MyceliumTool(val key: String, val label: String, val running: String) {
    Recall("recall", "Search memory", "Searching memory"),
    Remember("remember", "Remember", "Remembering"),
    ProposeTask("propose_task", "Propose task", "Proposing a task"),
    AskQuestion("ask_question", "Ask question", "Asking a question"),
    ProposeSchedule("propose_schedule", "Propose schedule", "Proposing a schedule"),
    Walkthrough("walkthrough", "Walkthrough", "Writing a walkthrough"),
    ProposeInstructionsEdit("propose_instructions_edit", "Propose instructions edit", "Proposing an instructions edit"),
    FindConversations("find_conversations", "Find conversations", "Finding conversations"),
    SendToConversation("send_to_conversation", "Send to conversation", "Proposing a message"),
}

sealed interface MyceliumCall {
    val tool: MyceliumTool

    data class Recall(val query: String, val memories: List<String>, val omitted: String?) : MyceliumCall {
        override val tool = MyceliumTool.Recall
    }

    data class Remember(val text: String, val sharedWith: String?) : MyceliumCall {
        override val tool = MyceliumTool.Remember
    }

    data class ProposeTask(val title: String, val body: String?, val project: String?) : MyceliumCall {
        override val tool = MyceliumTool.ProposeTask
    }

    data class AskQuestion(val question: String, val options: List<String>, val taskId: String? = null) : MyceliumCall {
        override val tool = MyceliumTool.AskQuestion
    }

    data class ProposeSchedule(val prompt: String, val schedule: String?) : MyceliumCall {
        override val tool = MyceliumTool.ProposeSchedule
    }

    data class Walkthrough(val steps: List<String>) : MyceliumCall {
        override val tool = MyceliumTool.Walkthrough
    }

    data class ProposeInstructionsEdit(val layer: String?, val reason: String) : MyceliumCall {
        override val tool = MyceliumTool.ProposeInstructionsEdit
    }

    data class FindConversations(val query: String?, val conversations: List<String>) : MyceliumCall {
        override val tool = MyceliumTool.FindConversations
    }

    data class SendToConversation(val target: String, val message: String, val reason: String, val proposalId: String? = null) : MyceliumCall {
        override val tool = MyceliumTool.SendToConversation
    }
}

private val toolName = Regex("mycelium[_.:/ -]+(${MyceliumTool.entries.joinToString("|") { it.key }})\\b", RegexOption.IGNORE_CASE)
private val recalledMemory = Regex("^[0-9a-fA-F-]{36}: (.*)$")
private val rememberedAs = Regex("^Remembered as \\S+?, shared with (.+)\\.$")
private val proposedLayer = Regex("made for the (.+) instructions\\.")
private val askedQuestion = Regex("^Question (\\S+) asked")
private val foundConversation = Regex("^[0-9a-fA-F-]{36}: (.*)$")
private val waitingToSend = Regex("^Waiting for the person to approve sending this to (.+?)(?: \\(proposal ([0-9a-fA-F-]{36})\\))?\\.$")
private val proposedSchedule = Regex("proposed \\((.+), ([^,]+)\\)\\.")

fun myceliumTool(title: String): MyceliumTool? {
    val key = toolName.find(title)?.groupValues?.get(1)?.lowercase()

    return MyceliumTool.entries.firstOrNull { it.key == key }
}

fun myceliumCall(call: ThreadItem.ToolCall): MyceliumCall? {
    val tool = myceliumTool(call.title) ?: return null
    val input = call.input
    val output = call.output.orEmpty()

    if (input == null && call.status == ToolStatus.Done) {
        return null
    }

    val fields = input ?: JsonObject(emptyMap())

    return when (tool) {
        MyceliumTool.Recall -> {
            val query = fields.requiredString("query", input) ?: return null
            val lines = output.lines().filter { it.isNotBlank() }
            val memories = lines.mapNotNull { recalledMemory.matchEntire(it)?.groupValues?.get(1) }
            val omitted = lines.lastOrNull { recalledMemory.matchEntire(it) == null && !it.startsWith("No memories match") }

            MyceliumCall.Recall(query, memories, omitted)
        }

        MyceliumTool.Remember -> {
            val text = fields.requiredString("text", input) ?: return null

            MyceliumCall.Remember(text, rememberedAs.matchEntire(output.trim())?.groupValues?.get(1))
        }

        MyceliumTool.ProposeTask -> {
            val title = fields.requiredString("title", input) ?: return null

            MyceliumCall.ProposeTask(title, fields.string("body")?.trim()?.ifEmpty { null }, fields.string("project")?.trim()?.ifEmpty { null })
        }

        MyceliumTool.AskQuestion -> {
            val question = fields.requiredString("question", input) ?: return null

            MyceliumCall.AskQuestion(question, fields.strings("options").orEmpty(), askedQuestion.find(output.trim())?.groupValues?.get(1))
        }

        MyceliumTool.ProposeSchedule -> {
            val prompt = fields.requiredString("prompt", input) ?: return null
            val result = proposedSchedule.find(output)
            val cron = result?.groupValues?.get(1) ?: fields.string("cron")
            val timeZone = result?.groupValues?.get(2) ?: fields.string("time_zone")
            val preset = fields.string("preset")
            val schedule = when {
                cron != null && (result != null || preset == null) -> if (timeZone != null) describeInWords(cron, timeZone) else describeCron(cron)
                preset != null -> if (timeZone != null) "$preset, $timeZone" else preset
                else -> null
            }

            MyceliumCall.ProposeSchedule(prompt, schedule)
        }

        MyceliumTool.Walkthrough -> {
            val steps = (fields["steps"] as? JsonArray)?.mapNotNull { (it as? JsonObject)?.string("title") }

            if (input != null && steps.isNullOrEmpty()) {
                return null
            }

            MyceliumCall.Walkthrough(steps.orEmpty())
        }

        MyceliumTool.ProposeInstructionsEdit -> {
            val reason = fields.requiredString("reason", input) ?: return null

            MyceliumCall.ProposeInstructionsEdit(proposedLayer.find(output)?.groupValues?.get(1) ?: fields.string("layer"), reason)
        }

        MyceliumTool.FindConversations -> {
            val conversations = output.lines().mapNotNull { foundConversation.matchEntire(it)?.groupValues?.get(1) }

            MyceliumCall.FindConversations(fields.string("query")?.trim()?.ifEmpty { null }, conversations)
        }

        MyceliumTool.SendToConversation -> {
            val message = fields.requiredString("message", input) ?: return null
            val waiting = waitingToSend.matchEntire(output.trim())
            val target = waiting?.groupValues?.get(1) ?: fields.string("conversation")?.trim().orEmpty()

            MyceliumCall.SendToConversation(target, message, fields.string("reason")?.trim().orEmpty(), waiting?.groupValues?.get(2)?.ifEmpty { null })
        }
    }
}

private fun JsonObject.requiredString(key: String, input: JsonObject?): String? {
    val value = string(key)?.trim()

    return if (input == null) value.orEmpty() else value?.ifEmpty { null }
}

private fun JsonObject.strings(key: String): List<String>? =
    (this[key] as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.takeIf { primitive -> primitive.isString }?.content }
