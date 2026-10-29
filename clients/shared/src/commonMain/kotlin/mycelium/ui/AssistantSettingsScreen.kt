package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.key
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
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
import mycelium.api.InstructionLayer
import mycelium.api.AssistantSecret
import mycelium.api.SetAssistantBackupRequest
import mycelium.api.PatchAssistantRequest
import mycelium.api.PutAssistantSecretRequest
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.NoGroup
import mycelium.state.SpeechSpeedNormal
import mycelium.state.Store
import mycelium.state.VoiceAssistantKey
import mycelium.state.ago
import mycelium.state.formatSize
import mycelium.state.displayName
import mycelium.state.modelOptions

enum class AssistantSection(val label: String, val icon: MyIcon) {
    Identity("Identity", MyIcon.Person),
    Instructions("Instructions", MyIcon.File),
    ModelsAndNodes("Models and nodes", MyIcon.Server),
    StorageAndSecrets("Storage and secrets", MyIcon.Key),
    SchedulesAndActions("Schedules and actions", MyIcon.Calendar),
}

@Composable
fun AssistantSettingsScreen(store: Store, state: AppState, assistantId: String, startOn: AssistantSection = AssistantSection.Identity, onDone: () -> Unit, onBack: () -> Unit) {
    var loaded by remember(assistantId) { mutableStateOf<Assistant?>(null) }
    var section by remember(assistantId) { mutableStateOf(startOn) }
    val runner = rememberAction(assistantId)
    val touch = LocalTouch.current

    LaunchedEffect(assistantId) {
        store.load { store.refreshGroups() }
        runner { loaded = store.api.assistant(assistantId) }
    }

    val current = loaded

    Column(Modifier.fillMaxSize()) {
        Bar {
            OpenListButton()
            BackButton(current?.name ?: "the assistant", onBack)
            current?.let { AssistantAvatar(it.name, builtIn = it.builtinKey != null, voice = it.builtinKey == VoiceAssistantKey) }
            BarTitle("${current?.name ?: "Assistant"} settings")

            if (current?.builtinKey != null) {
                Tag("Built in")
            }
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }, modifier = Modifier.padding(Tokens.Space.S3)) }

        if (touch) {
            TabRow(AssistantSection.entries.map { it.label }, section.label, { picked -> section = AssistantSection.entries.first { it.label == picked } }, "Assistant settings sections", Modifier.padding(horizontal = Tokens.Space.S3))
            Hairline()
        }

        Row(Modifier.weight(1f).fillMaxWidth()) {
            if (!touch) {
                Column(Modifier.width(Tokens.Size.SettingsNav).fillMaxHeight().verticalScrollWithBar().padding(Tokens.Space.S1_5).semantics { }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                    for (entry in AssistantSection.entries) {
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = rowHeight())
                                .pressable({ section = entry }, role = Role.Tab, selected = entry == section, shape = BlockShape)
                                .semantics { selected = entry == section }
                                .padding(horizontal = Tokens.Space.S2),
                            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(entry.icon, MyTheme.colors.textMuted)
                            Label(entry.label, maxLines = 1)
                        }
                    }
                }

                VerticalHairline()
            }

            Column(Modifier.weight(1f).fillMaxHeight().verticalScrollWithBar().padding(horizontal = Tokens.Space.S6, vertical = Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
                if (current != null) {
                    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
                        when (section) {
                            AssistantSection.Identity -> IdentitySection(store, state, current, onChanged = { loaded = it }, onArchived = onDone)
                            AssistantSection.Instructions -> InstructionsSection(store, current, onChanged = { loaded = it })
                            AssistantSection.ModelsAndNodes -> ModelsSection(store, state, current, onChanged = { loaded = it })
                            AssistantSection.StorageAndSecrets -> {
                                StorageSection(store, current.id)
                                SecretsSection(store, current.id)
                                BackupSection(store, state, current)
                            }

                            AssistantSection.SchedulesAndActions -> ActionsSection(store, state, current)
                        }
                    }
                }
            }
        }
    }
}

private val SpeechSpeeds = listOf("slower" to "Slower", SpeechSpeedNormal to "Normal", "faster" to "Faster")

@Composable
private fun SectionActions(onSave: () -> Unit, onCancel: () -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        MyButton(onSave, text = "Save", variant = ButtonVariant.Outline)
        MyButton(onCancel, text = "Cancel", muted = true)
    }
}

@Composable
private fun IdentitySection(store: Store, state: AppState, assistant: Assistant, onChanged: (Assistant) -> Unit, onArchived: () -> Unit) {
    val colors = MyTheme.colors
    var name by remember(assistant.id) { mutableStateOf(assistant.name) }
    var description by remember(assistant.id) { mutableStateOf(assistant.description.orEmpty()) }
    var group by remember(assistant.id) { mutableStateOf(assistant.groupId ?: NoGroup) }
    var nameError by remember(assistant.id) { mutableStateOf<String?>(null) }
    var confirmingArchive by remember(assistant.id) { mutableStateOf(false) }
    val runner = rememberAction(assistant.id)
    val builtIn = assistant.builtinKey != null
    val isVoice = assistant.builtinKey == VoiceAssistantKey
    val voices by store.voice.state.collectAsState()
    var voiceName by remember(assistant.id, assistant.preferences) { mutableStateOf(assistant.preferences.voiceName) }
    var speechSpeed by remember(assistant.id, assistant.preferences) { mutableStateOf(assistant.preferences.speechSpeed ?: SpeechSpeedNormal) }
    val groupName = state.groups[group]?.name
    val groupChoices = state.groups.values.sortedBy { it.name.lowercase() }.map { Choice(it.id, it.name) } + Choice(NoGroup, "No group")

    fun reset() {
        name = assistant.name
        description = assistant.description.orEmpty()
        group = assistant.groupId ?: NoGroup
        voiceName = assistant.preferences.voiceName
        speechSpeed = assistant.preferences.speechSpeed ?: SpeechSpeedNormal
        nameError = null
    }

    if (isVoice) {
        LaunchedEffect(Unit) {
            store.voice.loadVoices()
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        if (isVoice) {
            Label("${assistant.name} is built in. It cannot be renamed or archived.", color = colors.textMuted)
        } else if (builtIn) {
            Label("${assistant.name} is built in. It cannot be renamed or archived. Pause its schedules instead.", color = colors.textMuted)
        } else {
            TextInput(name, { name = it; nameError = null }, label = "Name", placeholder = "Stand-up", error = nameError, modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))
        }

        TextInput(description, { description = it }, label = "Description", placeholder = "Leave empty to use the first line of the instructions", modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Group", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            OptionMenu("Group", groupChoices, group, { group = it }, title = "Group", icon = MyIcon.Layers)

            if (groupName != null) {
                Label("Group instructions apply to its sessions, and the sidebar group filter shows it under $groupName.", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        if (isVoice) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Speaking voice", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                OptionMenu("Speaking voice", voices.voices.map { Choice(it, it) }, voiceName, { voiceName = it }, title = "Speaking voice", label = "Provider default", icon = MyIcon.Speaker)
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Speaking speed", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Segmented(SpeechSpeeds.map { it.second }, SpeechSpeeds.first { it.first == speechSpeed }.second, { picked -> speechSpeed = SpeechSpeeds.first { it.second == picked }.first }, groupLabel = "Speaking speed")
            }
        }

        SectionActions(
            onSave = {
                if (name.isBlank()) {
                    nameError = "Enter a name for the assistant."

                    return@SectionActions
                }

                runner {
                    val chosen = group.takeUnless { it == NoGroup }

                    val preferences = assistant.preferences.copy(voiceName = voiceName, speechSpeed = speechSpeed).takeIf { isVoice }

                    onChanged(store.patchAssistant(assistant, PatchAssistantRequest(name = name.takeUnless { builtIn }, description = description, groupId = chosen, clearGroup = chosen == null, preferences = preferences)))
                }
            },
            onCancel = ::reset,
        )

        if (isVoice) {
            Hairline()

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label("Turn voice off", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Hides the microphone. The Voice thread and its row stay. Change it any time in Privacy.", style = Tokens.Type.Meta, color = colors.textMuted)

                Row {
                    MyButton({ store.openScreen("settings/privacy") }, text = "Turn voice off", icon = MyIcon.Mode, variant = ButtonVariant.Outline)
                }
            }
        }

        if (!builtIn) {
            Hairline()

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label("Archive", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Archived assistants keep their thread, files and secrets, and stop waking for schedules. Restore one from Assistants, state Archived.", style = Tokens.Type.Meta, color = colors.textMuted)

                Row {
                    MyButton({ confirmingArchive = true }, text = "Archive assistant", icon = MyIcon.Archive, variant = ButtonVariant.Outline)
                }
            }
        }
    }

    if (confirmingArchive) {
        ConfirmDialog("Archive ${assistant.name}?", "It keeps its thread, files and secrets, and stops waking for schedules. You can restore it from Assistants.", "Archive assistant", onDismiss = { confirmingArchive = false }, onConfirm = {
            confirmingArchive = false
            runner {
                store.patchAssistant(assistant, PatchAssistantRequest(archived = true))
                onArchived()
            }
        })
    }
}

@Composable
private fun InstructionsSection(store: Store, assistant: Assistant, onChanged: (Assistant) -> Unit) {
    val colors = MyTheme.colors
    var editorKey by remember(assistant.id) { mutableStateOf(0) }
    var confirmingReset by remember(assistant.id) { mutableStateOf(false) }
    val runner = rememberAction(assistant.id)

    LaunchedEffect(assistant.id) {
        store.load { store.refreshInstructionProposals() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Instructions", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        Label("Kept in the assistant's storage as AGENTS.md and added after the global and group instructions.", style = Tokens.Type.Meta, color = colors.textMuted)
        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        key(editorKey, assistant.instructions) {
            InstructionsEditor(
                layer = InstructionLayer("assistant:${assistant.id}", assistant.name, text = assistant.instructions.orEmpty()),
                file = "AGENTS.md",
                saveLabel = "Save",
                onSave = { text -> runner { onChanged(store.patchAssistant(assistant, PatchAssistantRequest(instructions = text))) } },
                actions = {
                    MyButton({ editorKey += 1 }, text = "Cancel", muted = true)

                    if (assistant.builtinKey == VoiceAssistantKey) {
                        MyButton({ confirmingReset = true }, text = "Reset to default", icon = MyIcon.Refresh, muted = true)
                    }
                },
            )
        }

        if (confirmingReset) {
            ConfirmDialog("Reset ${assistant.name} to its default instructions?", "Your edited version stays in History, so you can bring it back.", "Reset to default", onDismiss = { confirmingReset = false }, onConfirm = {
                confirmingReset = false
                runner { onChanged(store.resetAssistantInstructions(assistant)) }
            })
        }

        Label("Proposed edits", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        InstructionProposals(store, layer = "assistant:${assistant.id}")
    }
}

@Composable
private fun ModelsSection(store: Store, state: AppState, assistant: Assistant, onChanged: (Assistant) -> Unit) {
    val colors = MyTheme.colors
    var models by remember(assistant.id) { mutableStateOf(assistant.preferences.models) }
    var nodes by remember(assistant.id) { mutableStateOf(assistant.preferences.nodes) }
    var chatFirst by remember(assistant.id) { mutableStateOf(assistant.preferences.chatFirst) }
    var fallBackOn by remember(assistant.id) { mutableStateOf(assistant.preferences.fallBackOn) }
    var providers by remember { mutableStateOf(emptyList<ModelProvider>()) }
    val runner = rememberAction(assistant.id)
    val favourites = rememberFavouriteModels(store)
    val options = remember(state.nodes, providers) { modelOptions(state.nodes.values, null, providers) }

    LaunchedEffect(Unit) {
        store.act { providers = store.api.modelProviders() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Models, in order of preference", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Label("A new session starts on the first model that can run. Any model picks from your favourites.", style = Tokens.Type.Meta, color = colors.textMuted)

            OrderedRows(
                names = models.map { it.displayName(options, providers) },
                onMove = { from, to -> models = models.moved(from, to) },
                onRemove = { index -> models = models.filterIndexed { position, _ -> position != index } },
            )

            Row {
                ModelPicker(
                    ModelPickerControls(
                        label = "Add a model",
                        options = options,
                        favourites = favourites.list,
                        providers = providers,
                        onPickModel = { picked -> models = models + picked.choice() },
                        onPickAny = { models = models + ModelChoice(anyModel = true) },
                        onFavourite = favourites.toggle,
                    ),
                )
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

            assistant.nextChoice?.let { Label("Next turn: $it", style = Tokens.Type.Meta, color = colors.textMuted) }
        }

        SectionActions(
            onSave = { runner { onChanged(store.patchAssistant(assistant, PatchAssistantRequest(preferences = assistant.preferences.copy(models = models, nodes = nodes, chatFirst = chatFirst, fallBackOn = fallBackOn)))) } },
            onCancel = {
                models = assistant.preferences.models
                nodes = assistant.preferences.nodes
                chatFirst = assistant.preferences.chatFirst
                fallBackOn = assistant.preferences.fallBackOn
            },
        )
    }
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
internal fun OrderedRows(names: List<String>, onMove: (Int, Int) -> Unit, onRemove: (Int) -> Unit, extra: (@Composable (Int) -> Unit)? = null) {
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
    var editing by remember(assistantId) { mutableStateOf<Pair<String, String>?>(null) }
    var newPath by remember(assistantId) { mutableStateOf("") }
    val runner = rememberAction(assistantId)

    suspend fun reload() {
        files = store.api.assistantFiles(assistantId).filter { it.path != InstructionsFile }
    }

    LaunchedEffect(assistantId) {
        runner { reload() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Storage", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        Label("Files this assistant keeps between sessions. It reads and writes them itself.", style = Tokens.Type.Meta, color = colors.textMuted)
        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

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
                                MenuItem("Edit", onClick = { close(); runner { editing = file.path to store.api.assistantFile(assistantId, file.path) } }, leading = MyIcon.Pencil)
                                MenuItem("Delete", onClick = { close(); runner { store.api.deleteAssistantFile(assistantId, file.path); reload() } }, leading = MyIcon.Close)
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
                onSave = { updated -> runner { store.api.saveAssistantFile(assistantId, path, updated); editing = null; reload() } },
                actions = { MyButton({ editing = null }, text = "Cancel", muted = true) },
                editLabel = "Edit $path",
            )
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
            TextInput(newPath, { newPath = it }, label = "New file path", placeholder = "notes/today.md", modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))
            MyButton(
                {
                    if (newPath.isBlank()) {
                        runner.failure = "Enter the path of the new file, for example notes/today.md."
                    } else {
                        runner.failure = null
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
    var setting by remember(assistantId) { mutableStateOf<String?>(null) }
    var newName by remember(assistantId) { mutableStateOf("") }
    var newDescription by remember(assistantId) { mutableStateOf("") }
    val runner = rememberAction(assistantId)

    suspend fun reload() {
        secrets = store.api.assistantSecrets(assistantId)
    }

    LaunchedEffect(assistantId) {
        runner { reload() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Secrets", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        Label("Reach working folder sessions as environment variables when they start. Values are never shown again.", style = Tokens.Type.Meta, color = colors.textMuted)
        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

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
                                MenuItem("Delete", onClick = { close(); runner { store.api.deleteAssistantSecret(assistantId, secret.name); reload() } }, leading = MyIcon.Close)
                            }
                        }
                    }
                },
            )
        }

        setting?.let { name ->
            SecretValueForm(name, onSave = { value -> runner { store.api.putAssistantSecret(assistantId, name, PutAssistantSecretRequest(value = value)); setting = null; reload() } }, onCancel = { setting = null })
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
            TextInput(newName, { newName = it }, label = "New secret name", placeholder = "GITHUB_TOKEN", mono = true, modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))
            TextInput(newDescription, { newDescription = it }, label = "What it is for", placeholder = "Pushes to the repository", modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))
            MyButton(
                {
                    runner {
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
    val runner = rememberAction(assistant.id)

    var forges by remember(assistant.id) { mutableStateOf(emptyList<mycelium.api.Forge>()) }

    LaunchedEffect(assistant.id) {
        runner { forges = store.api.forges() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Backup", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        Label("Commits changes to a repository after each turn. Files matched by the .gitignore in storage stay out.", style = Tokens.Type.Meta, color = colors.textMuted)
        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        if (backup != null) {
            Label(backup.repository, style = Tokens.Type.Code)
            Label(backup.error ?: backup.lastAt?.let { "Last backed up ${ago(it)}" } ?: "Not backed up yet", color = if (backup.error != null) colors.statusFailed else colors.textMuted)

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({ runner { store.api.pullAssistantBackup(assistant.id) } }, text = "Pull from repository", variant = ButtonVariant.Outline)
                MyButton({ runner { store.api.clearAssistantBackup(assistant.id) } }, text = "Stop backing up", variant = ButtonVariant.Outline)
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

                            runner { store.api.setAssistantBackup(assistant.id, SetAssistantBackupRequest(chosen, repository.trim())) }
                        },
                        text = "Back up to this repository",
                        variant = ButtonVariant.Outline,
                    )
                }
            }
        }
    }
}
