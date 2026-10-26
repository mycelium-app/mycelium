package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.Assistant
import mycelium.api.AssistantFile
import mycelium.api.AssistantPreferences
import mycelium.api.ModelChoice
import mycelium.api.ModelProvider
import mycelium.api.CreateAssistantRequest
import mycelium.api.InstructionLayer
import mycelium.api.PatchAssistantRequest
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.NoGroup
import mycelium.state.Store
import mycelium.state.ago
import mycelium.state.formatSize
import mycelium.state.harnessModels

@Composable
fun AssistantSettingsScreen(store: Store, state: AppState, assistantId: String?, onDone: () -> Unit) {
    val colors = MyTheme.colors
    var failure by remember { mutableStateOf<String?>(null) }
    var loaded by remember(assistantId) { mutableStateOf<Assistant?>(null) }
    var name by remember(assistantId) { mutableStateOf("") }
    var description by remember(assistantId) { mutableStateOf("") }
    var group by remember(assistantId) { mutableStateOf(NoGroup) }
    var models by remember(assistantId) { mutableStateOf(emptyList<ModelChoice>()) }
    var nodes by remember(assistantId) { mutableStateOf(emptyList<String>()) }
    var chatFirst by remember(assistantId) { mutableStateOf(true) }
    var fallBackOn by remember(assistantId) { mutableStateOf(AssistantPreferences().fallBackOn) }
    var providers by remember { mutableStateOf(emptyList<ModelProvider>()) }
    val attempt = rememberAttempt { failure = it }
    val creating = assistantId == null
    val ready = creating || loaded != null

    LaunchedEffect(assistantId) {
        store.act { store.refreshGroups() }
        store.act { providers = store.api.modelProviders() }

        if (assistantId != null) {
            attempt {
                val held = store.api.assistant(assistantId)

                name = held.name
                description = held.description.orEmpty()
                group = held.groupId ?: NoGroup
                models = held.preferences.models
                nodes = held.preferences.nodes
                chatFirst = held.preferences.chatFirst
                fallBackOn = held.preferences.fallBackOn
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
                        Label("Models, in order of preference", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                        Label("A new session starts on the first model.", style = Tokens.Type.Meta, color = colors.textMuted)

                        OrderedRows(
                            names = models.map { modelName(it, providers) },
                            onMove = { from, to -> models = models.moved(from, to) },
                            onRemove = { index -> models = models.filterIndexed { position, _ -> position != index } },
                        ) { index ->
                            val choice = models[index]

                            if (choice.agent != null) {
                                val offered = listOf(Choice("", "Default model")) + harnessModels(state.conversations.values, choice.agent)

                                OptionMenu("Model for ${harnessName(choice.agent)}", offered, choice.model ?: "", { picked -> models = models.mapIndexed { position, held -> if (position == index) held.copy(model = picked.ifEmpty { null }) else held } }, title = "Model")
                            }
                        }

                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                            ModelPicker(ModelPickerControls(providers, null, null) { provider, model -> models = models + ModelChoice(providerId = provider.id, model = model) })
                            MyButton({ models = models + ModelChoice(agent = "claude") }, text = "Add Claude Code", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
                            MyButton({ models = models + ModelChoice(agent = "codex") }, text = "Add Codex", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
                        }
                    }

                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Label("Runs on, in order of preference", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                        Label("Used when a session needs files or commands. Leave empty to use any node that is online.", style = Tokens.Type.Meta, color = colors.textMuted)

                        OrderedRows(
                            names = nodes.map { state.nodes[it]?.name ?: "Removed node" },
                            onMove = { from, to -> nodes = nodes.moved(from, to) },
                            onRemove = { index -> nodes = nodes.filterIndexed { position, _ -> position != index } },
                        )

                        val addable = state.nodes.values.filter { it.id !in nodes }.sortedBy { it.name.lowercase() }.map { Choice(it.id, it.name) }

                        if (addable.isNotEmpty()) {
                            Row {
                                OptionMenu("Add node", addable, null, { nodes = nodes + it }, title = "Add node", icon = MyIcon.Plus)
                            }
                        }
                    }

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(chatFirst, "Chat first when no files are needed", { chatFirst = !chatFirst }, checkedState = "On", uncheckedState = "Off")
                        Label("Chat first when no files are needed")
                    }

                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Label("Move to the next choice when", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                        for ((condition, label) in FallBackConditions) {
                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(condition in fallBackOn, label, { fallBackOn = if (condition in fallBackOn) fallBackOn - condition else fallBackOn + condition }, checkedState = "On", uncheckedState = "Off")
                                Label(label)
                            }
                        }

                        loaded?.nextChoice?.let { Label("Next turn: $it", style = Tokens.Type.Meta, color = colors.textMuted) }
                    }

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

                                    val held = loaded

                                    if (held == null) {
                                        store.createAssistant(CreateAssistantRequest(name, description.ifBlank { null }, chosen, text, AssistantPreferences(models = models, nodes = nodes, chatFirst = chatFirst, fallBackOn = fallBackOn).takeIf { models.isNotEmpty() }))
                                    } else {
                                        store.patchAssistant(held, PatchAssistantRequest(name = name, description = description, groupId = chosen, clearGroup = chosen == null, instructions = text, preferences = held.preferences.copy(models = models, nodes = nodes, chatFirst = chatFirst, fallBackOn = fallBackOn)))
                                    }

                                    onDone()
                                }
                            },
                            actions = { MyButton(onDone, text = "Cancel", muted = true) },
                        )
                    }

                    loaded?.let { StorageSection(store, it.id) }
                }
            }
        }
    }
}

private fun modelName(choice: ModelChoice, providers: List<ModelProvider>): String = when {
    choice.agent != null -> harnessName(choice.agent)
    else -> "${providers.firstOrNull { it.id == choice.providerId }?.name ?: "Removed provider"} · ${choice.model.orEmpty()}"
}

private val FallBackConditions = listOf(
    "usage_limit" to "A usage limit is reached",
    "signed_out" to "The account is signed out",
    "offline" to "The node is offline",
    "model_error" to "The model fails",
)

private fun harnessName(agent: String): String = if (agent == "codex") "Codex" else "Claude Code"

private fun <T> List<T>.moved(from: Int, to: Int): List<T> = toMutableList().also { it.add(to, it.removeAt(from)) }

@Composable
private fun OrderedRows(names: List<String>, onMove: (Int, Int) -> Unit, onRemove: (Int) -> Unit, extra: @Composable (Int) -> Unit = {}) {
    if (names.isEmpty()) {
        return
    }

    ContentTable(listOf(ColumnSpec(flexible = true), ColumnSpec(max = 200.dp), ColumnSpec(min = 0.dp, max = 28.dp))) {
        names.forEachIndexed { index, name ->
            ContentRow {
                Label(name, maxLines = 1)
                Box { extra(index) }

                RowMenu(name) { close ->
                    if (index > 0) {
                        MenuItem("Move up", onClick = { close(); onMove(index, index - 1) }, leading = MyIcon.ArrowUp)
                    }

                    if (index < names.lastIndex) {
                        MenuItem("Move down", onClick = { close(); onMove(index, index + 1) }, leading = MyIcon.ArrowDown)
                    }

                    MenuItem("Remove", onClick = { close(); onRemove(index) }, leading = MyIcon.Close)
                }
            }

            Hairline()
        }
    }
}

private const val InstructionsFile = "AGENTS.md"

@Composable
private fun StorageSection(store: Store, assistantId: String) {
    val colors = MyTheme.colors
    var files by remember(assistantId) { mutableStateOf(emptyList<AssistantFile>()) }
    var failure by remember(assistantId) { mutableStateOf<String?>(null) }
    var editing by remember(assistantId) { mutableStateOf<Pair<String, String>?>(null) }
    var newPath by remember(assistantId) { mutableStateOf("") }
    val attempt = rememberAttempt { failure = it }

    suspend fun reload() {
        files = store.api.assistantFiles(assistantId).filter { it.path != InstructionsFile }
    }

    LaunchedEffect(assistantId) {
        attempt { reload() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Storage", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        Label("Files this assistant keeps between sessions. It reads and writes them itself.", style = Tokens.Type.Meta, color = colors.textMuted)
        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (files.isNotEmpty()) {
            ContentTable(listOf(ColumnSpec(flexible = true), ColumnSpec(max = 100.dp), ColumnSpec(max = 120.dp), ColumnSpec(min = 0.dp, max = 28.dp))) {
                ContentRow(header = true) {
                    Label("Path", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Size", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Updated", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Box(Modifier)
                }

                Hairline()

                for (file in files) {
                    ContentRow {
                        Label(file.path, style = Tokens.Type.Code, maxLines = 1)
                        Label(formatSize(file.size), color = colors.textMuted, maxLines = 1)
                        Label(ago(file.updatedAt), color = colors.textMuted, maxLines = 1)

                        RowMenu(file.path) { close ->
                            MenuItem("Edit", onClick = { close(); attempt { editing = file.path to store.api.assistantFile(assistantId, file.path) } }, leading = MyIcon.Pencil)
                            MenuItem("Delete", onClick = { close(); attempt { store.api.deleteAssistantFile(assistantId, file.path); reload() } }, leading = MyIcon.Close)
                        }
                    }

                    Hairline()
                }
            }
        }

        editing?.let { (path, text) ->
            InstructionsEditor(
                layer = InstructionLayer(path, path, text = text),
                file = path,
                saveLabel = "Save file",
                onSave = { updated -> attempt { store.api.saveAssistantFile(assistantId, path, updated); editing = null; reload() } },
                actions = { MyButton({ editing = null }, text = "Cancel", muted = true) },
                editLabel = "Edit $path",
            )
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
            TextInput(newPath, { newPath = it }, label = "New file path", placeholder = "notes/today.md", modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))
            MyButton(
                {
                    if (newPath.isBlank()) {
                        failure = "Enter the path of the new file, for example notes/today.md."
                    } else {
                        failure = null
                        editing = newPath.trim() to ""
                        newPath = ""
                    }
                },
                text = "New file",
                icon = MyIcon.Plus,
                variant = ButtonVariant.Outline,
            )
        }
    }
}
