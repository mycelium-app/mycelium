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
import mycelium.api.AssistantSecret
import mycelium.api.SetAssistantBackupRequest
import mycelium.api.PatchAssistantRequest
import mycelium.api.PutAssistantSecretRequest
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.NoGroup
import mycelium.state.Store
import mycelium.state.ago
import mycelium.state.formatSize
import mycelium.state.harnessModels

@Composable
fun AssistantSettingsScreen(store: Store, state: AppState, assistantId: String?, onDone: () -> Unit, onBack: () -> Unit) {
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
            BackButton(if (creating) "assistants" else loaded?.name ?: "the assistant", onBack)
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
                    loaded?.let { SecretsSection(store, it.id) }
                    loaded?.let { BackupSection(store, state, it) }
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
private fun OrderedRows(names: List<String>, onMove: (Int, Int) -> Unit, onRemove: (Int) -> Unit, extra: (@Composable (Int) -> Unit)? = null) {
    if (names.isEmpty()) {
        return
    }

    Table(
        listOfNotNull(Col("", 140.dp, weight = 1f), extra?.let { Col("", 100.dp, preferred = 180.dp) }, ActionColumn),
        names.size,
        { index, column ->
            val name = names[index]

            when {
                column == 0 -> TableCell.Text(name)
                column == 1 && extra != null -> TableCell.Slot { extra(index) }
                else -> TableCell.Slot {
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
            }
        },
        header = false,
    )
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
            val updated = files.map { ago(it.updatedAt) }

            Table(
                listOf(
                    Col("Path", 140.dp, weight = 1f),
                    Col("Size", 56.dp, numeric = true),
                    Col("Updated", 88.dp, priority = 1),
                    Col("Backed up", 80.dp, priority = 1),
                    ActionColumn,
                ),
                files.size,
                { row, column ->
                    val file = files[row]

                    when (column) {
                        0 -> TableCell.Text(file.path, Tokens.Type.Code)
                        1 -> TableCell.Text(formatSize(file.size), color = colors.textMuted)
                        2 -> TableCell.Text(updated[row], color = colors.textMuted)
                        3 -> TableCell.Text(if (file.backedUp) "Yes" else "No", color = colors.textMuted)
                        else -> TableCell.Slot {
                            RowMenu(file.path) { close ->
                                MenuItem("Edit", onClick = { close(); attempt { editing = file.path to store.api.assistantFile(assistantId, file.path) } }, leading = MyIcon.Pencil)
                                MenuItem("Delete", onClick = { close(); attempt { store.api.deleteAssistantFile(assistantId, file.path); reload() } }, leading = MyIcon.Close)
                            }
                        }
                    }
                },
            )
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

@Composable
private fun SecretsSection(store: Store, assistantId: String) {
    val colors = MyTheme.colors
    var secrets by remember(assistantId) { mutableStateOf(emptyList<AssistantSecret>()) }
    var failure by remember(assistantId) { mutableStateOf<String?>(null) }
    var setting by remember(assistantId) { mutableStateOf<String?>(null) }
    var newName by remember(assistantId) { mutableStateOf("") }
    var newDescription by remember(assistantId) { mutableStateOf("") }
    val attempt = rememberAttempt { failure = it }

    suspend fun reload() {
        secrets = store.api.assistantSecrets(assistantId)
    }

    LaunchedEffect(assistantId) {
        attempt { reload() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Secrets", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        Label("Reach working folder sessions as environment variables when they start. Values are never shown again.", style = Tokens.Type.Meta, color = colors.textMuted)
        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (secrets.isNotEmpty()) {
            Table(
                listOf(
                    Col("Name", 120.dp, rigid = true),
                    Col("Description", 120.dp, weight = 1f, priority = 1),
                    Col("State", 72.dp),
                    Col("Set by", 80.dp, priority = 1),
                    ActionColumn,
                ),
                secrets.size,
                { row, column ->
                    val secret = secrets[row]

                    when (column) {
                        0 -> TableCell.Text(secret.name, Tokens.Type.Code)
                        1 -> TableCell.Text(secret.description.orEmpty(), color = colors.textMuted)
                        2 -> TableCell.Text(if (secret.state == "set") "Set" else "Waiting", color = colors.textMuted)
                        3 -> TableCell.Text(secret.setBy.orEmpty(), color = colors.textMuted)
                        else -> TableCell.Slot {
                            RowMenu(secret.name) { close ->
                                MenuItem("Set value", onClick = { close(); setting = secret.name }, leading = MyIcon.Pencil)
                                MenuItem("Delete", onClick = { close(); attempt { store.api.deleteAssistantSecret(assistantId, secret.name); reload() } }, leading = MyIcon.Close)
                            }
                        }
                    }
                },
            )
        }

        setting?.let { name ->
            SecretValueForm(name, onSave = { value -> attempt { store.api.putAssistantSecret(assistantId, name, PutAssistantSecretRequest(value = value)); setting = null; reload() } }, onCancel = { setting = null })
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
            TextInput(newName, { newName = it }, label = "New secret name", placeholder = "GITHUB_TOKEN", mono = true, modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))
            TextInput(newDescription, { newDescription = it }, label = "What it is for", placeholder = "Pushes to the repository", modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))
            MyButton(
                {
                    attempt {
                        store.api.putAssistantSecret(assistantId, newName.trim(), PutAssistantSecretRequest(description = newDescription.ifBlank { null }))
                        setting = newName.trim()
                        newName = ""
                        newDescription = ""
                        reload()
                    }
                },
                text = "Add secret",
                icon = MyIcon.Plus,
                variant = ButtonVariant.Outline,
            )
        }
    }
}

@Composable
private fun BackupSection(store: Store, state: AppState, assistant: Assistant) {
    val colors = MyTheme.colors
    val backup = state.assistants[assistant.id]?.backup ?: assistant.backup
    var forge by remember(assistant.id) { mutableStateOf<String?>(null) }
    var repository by remember(assistant.id) { mutableStateOf("") }
    var failure by remember(assistant.id) { mutableStateOf<String?>(null) }
    val attempt = rememberAttempt { failure = it }

    var forges by remember(assistant.id) { mutableStateOf(emptyList<mycelium.api.Forge>()) }

    LaunchedEffect(assistant.id) {
        attempt { forges = store.api.forges() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Backup", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        Label("Commits changes to a repository after each turn. Files matched by the .gitignore in storage stay out.", style = Tokens.Type.Meta, color = colors.textMuted)
        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (backup != null) {
            Label(backup.repository, style = Tokens.Type.Code)
            Label(backup.error ?: backup.lastAt?.let { "Last backed up ${ago(it)}" } ?: "Not backed up yet", color = if (backup.error != null) colors.statusFailed else colors.textMuted)

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({ attempt { store.api.pullAssistantBackup(assistant.id) } }, text = "Pull from repository", variant = ButtonVariant.Outline)
                MyButton({ attempt { store.api.clearAssistantBackup(assistant.id) } }, text = "Stop backing up", variant = ButtonVariant.Outline)
            }
        } else {
            if (forges.isEmpty()) {
                Label("Connect a forge in Settings, Forges to choose a repository.", color = colors.textMuted)
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
                    OptionMenu("Forge", forges.map { Choice(it.id, "${it.account} on ${it.baseUrl}") }, forge, { forge = it }, title = "Forge")
                    TextInput(repository, { repository = it }, label = "Repository", placeholder = "owner/name", mono = true, modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))
                    MyButton(
                        {
                            val chosen = forge ?: forges.first().id

                            attempt { store.api.setAssistantBackup(assistant.id, SetAssistantBackupRequest(chosen, repository.trim())) }
                        },
                        text = "Back up to this repository",
                        variant = ButtonVariant.Outline,
                    )
                }
            }
        }
    }
}
