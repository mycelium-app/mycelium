package mycelium.api

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject

sealed interface StreamMessage {
    data class ConversationChanged(val conversation: Conversation) : StreamMessage

    data class EventAppended(val event: Event) : StreamMessage

    data class ConversationRemoved(val conversationId: String) : StreamMessage

    data class QueueChanged(val conversationId: String, val queue: List<QueuedPrompt>) : StreamMessage

    data class MoveChanged(val progress: MoveProgress) : StreamMessage

    data class GoalChanged(val goal: Goal) : StreamMessage

    data class GoalCleared(val conversationId: String) : StreamMessage

    data class WalkthroughChanged(val walkthrough: Walkthrough) : StreamMessage

    data class WalkthroughNotesChanged(val conversationId: String, val notes: List<WalkthroughNote>) : StreamMessage

    data class WorkflowChanged(val workflow: Workflow) : StreamMessage

    data class ScheduleChanged(val schedule: Schedule) : StreamMessage

    data class AssistantChanged(val assistant: Assistant) : StreamMessage

    data class AssistantActionsChanged(val assistantId: String) : StreamMessage

    data class ActionProposalChanged(val proposal: ActionProposal) : StreamMessage

    data class AssistantChangeChanged(val change: AssistantChange) : StreamMessage

    data class ScheduleRemoved(val scheduleId: String) : StreamMessage

    data class ArtifactChanged(val artifact: Artifact, val deleted: Boolean = false) : StreamMessage

    data class ArtifactCommentAdded(val artifactId: String) : StreamMessage
    data class ArtifactAccessRequestChanged(val request: ArtifactAccessRequest) : StreamMessage

    data class InstructionProposalChanged(val proposal: InstructionProposal) : StreamMessage

    data class MessageProposalChanged(val proposal: MessageProposal) : StreamMessage

    data class StartProposalChanged(val proposal: StartProposal) : StreamMessage

    data class ConversationConnectionChanged(val connection: ConversationConnection) : StreamMessage

    data class GroupConnectionChanged(val connection: GroupConnection) : StreamMessage

    data class ApiAccessRequestChanged(val request: ApiAccessRequest) : StreamMessage

    data class SshAccessRequestChanged(val request: SshAccessRequest) : StreamMessage

    data class MemoryChanged(val memory: Memory) : StreamMessage

    data class MemoryRemoved(val memoryId: String) : StreamMessage

    data class MemoryBankChanged(val bank: MemoryBank) : StreamMessage

    data class MemoryBankRemoved(val bankId: String) : StreamMessage

    data class NodeChanged(val node: NodeSummary) : StreamMessage

    data class NodeRemoved(val nodeId: String) : StreamMessage

    data object IntegrationsChanged : StreamMessage

    data object WellnessChanged : StreamMessage

    data class ScreenOpened(val route: String) : StreamMessage

    data class NodeResourcesSampled(val nodeId: String, val sample: ResourceSample) : StreamMessage

    data class LoginOutput(val loginId: String, val data: String) : StreamMessage

    data class LoginFinished(val loginId: String, val ok: Boolean) : StreamMessage

    data class TerminalOutput(val terminalId: String, val data: String) : StreamMessage

    data class TerminalExited(val terminalId: String) : StreamMessage

    data class EnvironmentChanged(val environment: Environment) : StreamMessage

    data class EnvironmentActed(val conversationId: String, val action: EnvironmentAction) : StreamMessage
}

fun parseStreamMessage(text: String): StreamMessage? {
    val element = MyceliumJson.parseToJsonElement(text).jsonObject
    val type = (element["type"] as? JsonPrimitive)?.contentOrNull

    return when (type) {
        "conversation" -> StreamMessage.ConversationChanged(MyceliumJson.decodeFromJsonElement(Conversation.serializer(), element))
        "conversation_removed" -> StreamMessage.ConversationRemoved(element.requireString("conversation_id"))
        "event" -> StreamMessage.EventAppended(MyceliumJson.decodeFromJsonElement(Event.serializer(), element))
        "queue" -> StreamMessage.QueueChanged(
            conversationId = element.requireString("conversation_id"),
            queue = MyceliumJson.decodeFromJsonElement(
                kotlinx.serialization.builtins.ListSerializer(QueuedPrompt.serializer()),
                element.getValue("queue"),
            ),
        )
        "move" -> StreamMessage.MoveChanged(MyceliumJson.decodeFromJsonElement(MoveProgress.serializer(), element))
        "goal" -> StreamMessage.GoalChanged(MyceliumJson.decodeFromJsonElement(Goal.serializer(), element))
        "goal_cleared" -> StreamMessage.GoalCleared(element.requireString("conversation_id"))
        "walkthrough" -> StreamMessage.WalkthroughChanged(MyceliumJson.decodeFromJsonElement(Walkthrough.serializer(), element))
        "walkthrough_notes" -> StreamMessage.WalkthroughNotesChanged(
            conversationId = element.requireString("conversation_id"),
            notes = MyceliumJson.decodeFromJsonElement(
                kotlinx.serialization.builtins.ListSerializer(WalkthroughNote.serializer()),
                element.getValue("notes"),
            ),
        )
        "workflow" -> StreamMessage.WorkflowChanged(MyceliumJson.decodeFromJsonElement(Workflow.serializer(), element["workflow"]!!))
        "assistant" -> StreamMessage.AssistantChanged(MyceliumJson.decodeFromJsonElement(Assistant.serializer(), element))
        "assistant_actions_changed" -> StreamMessage.AssistantActionsChanged(element.requireString("assistant_id"))
        "action_proposal" -> StreamMessage.ActionProposalChanged(MyceliumJson.decodeFromJsonElement(ActionProposal.serializer(), element))
        "assistant_change" -> StreamMessage.AssistantChangeChanged(MyceliumJson.decodeFromJsonElement(AssistantChange.serializer(), element))
        "schedule" -> StreamMessage.ScheduleChanged(MyceliumJson.decodeFromJsonElement(Schedule.serializer(), element))
        "schedule_removed" -> StreamMessage.ScheduleRemoved(element.requireString("schedule_id"))
        "artifact" -> StreamMessage.ArtifactChanged(
            MyceliumJson.decodeFromJsonElement(Artifact.serializer(), element),
            deleted = (element["deleted"] as? JsonPrimitive)?.contentOrNull == "true",
        )
        "artifact_access_request" -> StreamMessage.ArtifactAccessRequestChanged(MyceliumJson.decodeFromJsonElement(ArtifactAccessRequest.serializer(), element))
        "artifact_comment" -> StreamMessage.ArtifactCommentAdded(element.requireString("artifact_id"))
        "instruction_proposal" -> StreamMessage.InstructionProposalChanged(MyceliumJson.decodeFromJsonElement(InstructionProposal.serializer(), element))
        "conversation_connection" -> StreamMessage.ConversationConnectionChanged(MyceliumJson.decodeFromJsonElement(ConversationConnection.serializer(), element))
        "group_connection" -> StreamMessage.GroupConnectionChanged(MyceliumJson.decodeFromJsonElement(GroupConnection.serializer(), element))
        "message_proposal" -> StreamMessage.MessageProposalChanged(MyceliumJson.decodeFromJsonElement(MessageProposal.serializer(), element))
        "start_proposal" -> StreamMessage.StartProposalChanged(MyceliumJson.decodeFromJsonElement(StartProposal.serializer(), element))
        "api_access_request" -> StreamMessage.ApiAccessRequestChanged(MyceliumJson.decodeFromJsonElement(ApiAccessRequest.serializer(), element))
        "ssh_access_request" -> StreamMessage.SshAccessRequestChanged(MyceliumJson.decodeFromJsonElement(SshAccessRequest.serializer(), element))
        "memory" -> StreamMessage.MemoryChanged(MyceliumJson.decodeFromJsonElement(Memory.serializer(), element))
        "memory_removed" -> StreamMessage.MemoryRemoved(element.requireString("memory_id"))
        "memory_bank" -> StreamMessage.MemoryBankChanged(MyceliumJson.decodeFromJsonElement(MemoryBank.serializer(), element))
        "memory_bank_removed" -> StreamMessage.MemoryBankRemoved(element.requireString("bank_id"))
        "node" -> StreamMessage.NodeChanged(MyceliumJson.decodeFromJsonElement(NodeSummary.serializer(), element))
        "node_removed" -> StreamMessage.NodeRemoved(element.requireString("node_id"))
        "integrations_changed" -> StreamMessage.IntegrationsChanged
        "wellness_changed" -> StreamMessage.WellnessChanged
        "open_screen" -> StreamMessage.ScreenOpened(element.requireString("route"))
        "node_resources" -> StreamMessage.NodeResourcesSampled(
            element.requireString("node_id"),
            MyceliumJson.decodeFromJsonElement(ResourceSample.serializer(), element.getValue("sample")),
        )
        "login_output" -> StreamMessage.LoginOutput(element.requireString("login_id"), element.requireString("data"))
        "login_finished" -> StreamMessage.LoginFinished(
            loginId = element.requireString("login_id"),
            ok = (element["ok"] as? JsonPrimitive)?.contentOrNull == "true",
        )
        "terminal_output" -> StreamMessage.TerminalOutput(element.requireString("terminal_id"), element.requireString("data"))
        "terminal_exited" -> StreamMessage.TerminalExited(element.requireString("terminal_id"))
        "environment" -> StreamMessage.EnvironmentChanged(MyceliumJson.decodeFromJsonElement(Environment.serializer(), element))
        "environment_action" -> StreamMessage.EnvironmentActed(
            element.requireString("conversation_id"),
            MyceliumJson.decodeFromJsonElement(EnvironmentAction.serializer(), element.getValue("action")),
        )
        else -> null
    }
}

private fun JsonObject.requireString(key: String): String =
    (this[key] as? JsonPrimitive)?.contentOrNull ?: error("Stream message is missing $key")
