package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.state.Store

@Composable
fun ExperimentsSettings(store: Store) {
    val state by store.state.collectAsState()
    val held = state.experiments
    val choices: List<Triple<String, Boolean, (Boolean) -> Unit>> = listOf(
        Triple("Tasks", held.tasks) { value -> store.updateExperiments { it.copy(tasks = value) } },
        Triple("Workflows", held.workflows) { value -> store.updateExperiments { it.copy(workflows = value) } },
        Triple("Instructions", held.instructions) { value -> store.updateExperiments { it.copy(instructions = value) } },
        Triple("Groups", held.groups) { value -> store.updateExperiments { it.copy(groups = value) } },
        Triple("Details sidebar", held.details) { value -> store.updateExperiments { it.copy(details = value) } },
        Triple("Subagents sidebar", held.subagents) { value -> store.updateExperiments { it.copy(subagents = value) } },
        Triple("Walkthrough", held.walkthrough) { value -> store.updateExperiments { it.copy(walkthrough = value) } },
        Triple("Move chats between machines", held.moveChats) { value -> store.updateExperiments { it.copy(moveChats = value) } },
        Triple("Continue elsewhere", held.continueElsewhere) { value -> store.updateExperiments { it.copy(continueElsewhere = value) } },
        Triple("Assistants", held.assistants) { value -> store.updateExperiments { it.copy(assistants = value) } },
    )

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Experiments", Modifier.semantics { heading() }, Tokens.Type.Title)

        for ((label, on, save) in choices) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Checkbox(on, label, { save(!on) }, checkedState = "On", uncheckedState = "Off")
                Label(label)
            }
        }
    }
}
