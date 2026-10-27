package mycelium.state

import kotlinx.serialization.json.JsonObject
import mycelium.api.Artifact
import mycelium.api.Schedule
import mycelium.api.Workflow

fun artifactInGroup(artifact: Artifact, state: AppState, filter: String): Boolean {
    val conversations = listOfNotNull(artifact.conversationId) + artifact.conversationIds.orEmpty()
    val groups = artifact.projectIds.orEmpty().map { state.projects[it]?.groupId } + conversations.map { state.conversations[it]?.groupId }

    return groups.ifEmpty { listOf(null) }.any { inGroup(it, filter) }
}

fun scheduleInGroup(schedule: Schedule, state: AppState, filter: String): Boolean {
    val group = schedule.conversationId?.let { state.conversations[it]?.groupId } ?: schedule.projectId?.let { state.projects[it]?.groupId }

    return inGroup(group, filter)
}

fun workflowInGroup(workflow: Workflow, state: AppState, filter: String): Boolean =
    inGroup(workflow.conversationId?.let { state.conversations[it]?.groupId }, filter)
