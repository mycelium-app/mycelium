package mycelium.ui

import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import mycelium.state.ActivityKind
import mycelium.state.ActivityTarget
import mycelium.state.AppState
import mycelium.state.WellnessStore
import mycelium.state.isChat

private fun elsewhereLabel(route: Route): String = when (route) {
    Route.New, is Route.NewIn -> "New conversation"
    Route.NewChat -> "New chat"
    Route.Nodes, is Route.Node -> "Nodes"
    Route.Tasks -> "Tasks"
    Route.Workflows, is Route.Workflow -> "Workflows"
    Route.Schedules -> "Schedules"
    Route.Wellness -> "Wellness & Learning"
    Route.Projects -> "Projects"
    Route.Issues -> "Issues"
    Route.Archive -> "History"
    Route.Artifacts, is Route.Artefact -> "Artefacts"
    Route.Activity -> "Activity"
    Route.UsageHistory -> "Usage"
    Route.Assistants, Route.NewAssistant, is Route.AssistantSettings -> "Assistants"
    Route.Settings -> "Settings"
    else -> "Mycelium"
}

fun activityTargetOf(route: Route, state: AppState): ActivityTarget = when (route) {
    is Route.Chat -> {
        val conversation = state.conversations[route.id]
        val kind = if (conversation?.isChat == true) ActivityKind.Chat else ActivityKind.Conversation

        ActivityTarget(kind, "conversation", conversation?.title, conversation?.groupId, route.id)
    }
    is Route.Assistant -> {
        val assistant = state.assistants[route.id]

        ActivityTarget(ActivityKind.Assistant, "assistant", assistant?.name, assistant?.groupId, assistant?.conversationId, route.id)
    }
    else -> {
        val label = elsewhereLabel(route)

        ActivityTarget(ActivityKind.Elsewhere, label.lowercase().replace(' ', '-'), label)
    }
}

fun Modifier.noticingInput(wellness: WellnessStore): Modifier =
    pointerInput(wellness) {
        awaitPointerEventScope {
            while (true) {
                awaitPointerEvent(PointerEventPass.Initial)
                wellness.noteInput()
            }
        }
    }.onPreviewKeyEvent {
        wellness.noteInput()
        false
    }
