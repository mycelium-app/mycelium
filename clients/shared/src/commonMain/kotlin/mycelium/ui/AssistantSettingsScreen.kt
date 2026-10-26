package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.Assistant
import mycelium.api.CreateAssistantRequest
import mycelium.api.InstructionLayer
import mycelium.api.PatchAssistantRequest
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.NoGroup
import mycelium.state.Store

@Composable
fun AssistantSettingsScreen(store: Store, state: AppState, assistantId: String?, onDone: () -> Unit) {
    val colors = MyTheme.colors
    var failure by remember { mutableStateOf<String?>(null) }
    var loaded by remember(assistantId) { mutableStateOf<Assistant?>(null) }
    var name by remember(assistantId) { mutableStateOf("") }
    var description by remember(assistantId) { mutableStateOf("") }
    var group by remember(assistantId) { mutableStateOf(NoGroup) }
    val attempt = rememberAttempt { failure = it }
    val creating = assistantId == null
    val ready = creating || loaded != null

    LaunchedEffect(assistantId) {
        store.act { store.refreshGroups() }

        if (assistantId != null) {
            attempt {
                val held = store.api.assistant(assistantId)

                name = held.name
                description = held.description.orEmpty()
                group = held.groupId ?: NoGroup
                loaded = held
            }
        }
    }

    val groupChoices = state.groups.values.sortedBy { it.name.lowercase() }.map { Choice(it.id, it.name) } + Choice(NoGroup, "No group")

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle(if (creating) "New assistant" else "${loaded?.name ?: "Assistant"} settings")
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
                failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

                if (ready) {
                    TextInput(name, { name = it }, label = "Name", placeholder = "Stand-up", modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))

                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Label("Group", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                        OptionMenu("Group", groupChoices, group, { group = it }, title = "Group", icon = MyIcon.Layers)
                    }

                    TextInput(description, { description = it }, label = "Description", placeholder = "Leave empty to use the first line of the instructions", modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))

                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Label("Instructions", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                        Label("Kept in the assistant's storage as AGENTS.md and added after the global and group instructions.", style = Tokens.Type.Meta, color = colors.textMuted)

                        InstructionsEditor(
                            layer = InstructionLayer("assistant", name.ifBlank { "assistant" }, text = loaded?.instructions.orEmpty()),
                            file = "AGENTS.md",
                            saveLabel = if (creating) "Create assistant" else "Save",
                            onSave = { text ->
                                attempt {
                                    val chosen = group.takeUnless { it == NoGroup }

                                    if (loaded == null) {
                                        store.createAssistant(CreateAssistantRequest(name, description.ifBlank { null }, chosen, text))
                                    } else {
                                        store.patchAssistant(loaded!!, PatchAssistantRequest(name = name, description = description, groupId = chosen, clearGroup = chosen == null, instructions = text))
                                    }

                                    onDone()
                                }
                            },
                            actions = { MyButton(onDone, text = "Cancel", muted = true) },
                        )
                    }
                }
            }
        }
    }
}
