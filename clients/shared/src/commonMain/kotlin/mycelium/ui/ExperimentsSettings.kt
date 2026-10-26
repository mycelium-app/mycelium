package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
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
    val choices = listOf(
        Experiment("Tasks", "A board of proposed and running work, and the agent tools to propose tasks and ask questions.", held.tasks) { value -> store.updateExperiments { it.copy(tasks = value) } },
        Experiment("Workflows", "Multi-step workflows across agents, models and nodes, and the agent tool that starts them.", held.workflows) { value -> store.updateExperiments { it.copy(workflows = value) } },
        Experiment("Instructions", "Edit the instructions agents receive, and review edits agents propose.", held.instructions) { value -> store.updateExperiments { it.copy(instructions = value) } },
        Experiment("Groups", "Give each group its own instructions and folders in Settings, Groups.", held.groups) { value -> store.updateExperiments { it.copy(groups = value) } },
        Experiment("Details sidebar", "A panel beside the conversation with its node, folder, account and usage.", held.details) { value -> store.updateExperiments { it.copy(details = value) } },
        Experiment("Subagents sidebar", "A panel listing the subagents a conversation started.", held.subagents) { value -> store.updateExperiments { it.copy(subagents = value) } },
        Experiment("Walkthrough", "Step-by-step tours of a change, written by the agent for you to read.", held.walkthrough) { value -> store.updateExperiments { it.copy(walkthrough = value) } },
        Experiment("Move chats between machines", "Move a conversation and its transcript to another node.", held.moveChats) { value -> store.updateExperiments { it.copy(moveChats = value) } },
        Experiment("Continue elsewhere", "Carry a conversation on with another account, harness or chat model.", held.continueElsewhere) { value -> store.updateExperiments { it.copy(continueElsewhere = value) } },
        Experiment("Assistants", "Persistent assistants with their own instructions, storage and secrets.", held.assistants) { value -> store.updateExperiments { it.copy(assistants = value) } },
        Experiment("Memory banks", "Keep memories apart by model class and group.", held.memoryBanks) { value -> store.updateExperiments { it.copy(memoryBanks = value) } },
        Experiment("Chat across groups", "Let conversations in connected groups message each other.", held.chatAcrossGroups) { value -> store.updateExperiments { it.copy(chatAcrossGroups = value) } },
        Experiment("Voice input", "Dictate into the composer. On Android and the web, audio may go to Google.", held.voice) { value -> store.updateExperiments { it.copy(voice = value) } },
    )

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Experiments", Modifier.semantics { heading() }, Tokens.Type.Title)

        for (experiment in choices) {
            Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Checkbox(experiment.on, experiment.name, { experiment.save(!experiment.on) }, checkedState = "On", uncheckedState = "Off")
                Column(Modifier.padding(top = Tokens.Space.S1_5)) {
                    Label(experiment.name)
                    Label(experiment.description, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
                }
            }
        }
    }
}

private class Experiment(val name: String, val description: String, val on: Boolean, val save: (Boolean) -> Unit)
