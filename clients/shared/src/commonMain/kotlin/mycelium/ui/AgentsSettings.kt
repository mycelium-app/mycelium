package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
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
import mycelium.api.AgentSettings
import mycelium.api.LoginState
import mycelium.api.ModelChoice
import mycelium.api.ModelProvider
import mycelium.api.NodePreference
import mycelium.api.SaveAgentSettingsRequest
import mycelium.api.SetAccountModeRequest
import mycelium.api.UsageLimitSetting
import mycelium.state.AgentAccount
import mycelium.state.Choice
import mycelium.state.ModelOption
import mycelium.state.Store
import mycelium.state.agentAccounts
import mycelium.state.agentName
import mycelium.state.displayName
import mycelium.state.modelOptions
import mycelium.state.splitSignedOut
import mycelium.state.withFavourite

private val UsageLimitChoices = listOf("Ask me" to "ask", "Wait and resume automatically" to "resume")

private val DefaultModeNameWidth = 120.dp

private const val SameAsAgent = ""

private fun defaultModeOf(settings: AgentSettings, agent: String): String? = if (agent == "codex") settings.defaultModes.codex else settings.defaultModes.claude

private fun accountModeOf(settings: AgentSettings, account: AgentAccount): String? = settings.accountModes.firstOrNull { it.agent == account.agent && it.account == account.label }?.mode

private fun entryHarness(entry: ModelChoice): String = if (entry.anyModel) "" else entry.agent?.let(::agentName) ?: "Chat"

private fun modeName(agent: String, id: String?): String? = fallbackModes(agent).firstOrNull { it.id == id }?.name

@Composable
fun AgentsSettings(store: Store) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var onUsageLimit by remember { mutableStateOf<String?>(null) }
    var settings by remember { mutableStateOf<AgentSettings?>(null) }
    var providers by remember { mutableStateOf(emptyList<ModelProvider>()) }
    var changingAccount by remember { mutableStateOf<AgentAccount?>(null) }
    var editingList by remember { mutableStateOf(false) }
    val options = modelOptions(state.nodes.values, null, providers)
    val runner = rememberAction()
    val accounts = splitSignedOut(agentAccounts(state.nodes.values)).accounts.filter { account -> account.nodes.any { it.state == LoginState.SignedIn } }

    LaunchedEffect(Unit) {
        store.act {
            onUsageLimit = store.api.usageLimitSetting().onUsageLimit
            settings = store.api.agentSettings()
            providers = store.api.modelProviders()
        }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        settings?.let { current ->
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label("Default mode", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("New conversations start in this mode instead of the one in ~/.claude or ~/.codex.", style = Tokens.Type.Meta, color = colors.textMuted)
                Label("Environment tools never ask, in any mode: they only reach the environment.", style = Tokens.Type.Meta, color = colors.textMuted)

                for (agent in listOf("claude", "codex")) {
                    val modes = fallbackModes(agent)

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label(agentName(agent), Modifier.width(DefaultModeNameWidth))
                        Segmented(
                            modes.map { it.name },
                            modeName(agent, defaultModeOf(current, agent)).orEmpty(),
                            { label ->
                                val picked = modes.first { it.name == label }.id
                                val defaults = if (agent == "codex") current.defaultModes.copy(codex = picked) else current.defaultModes.copy(claude = picked)

                                runner { settings = store.api.saveAgentSettings(SaveAgentSettingsRequest(defaults, current.anyModel)) }
                            },
                            groupLabel = "Default mode for ${agentName(agent)}",
                        )
                    }
                }
            }

            if (accounts.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label("Default mode for each account", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                    Table(
                        listOf(Col("Account", 120.dp, weight = 1f), Col("Agent", 90.dp), Col("Default mode", 140.dp, preferred = 200.dp), ActionColumn),
                        accounts.size,
                        { row, column ->
                            val account = accounts[row]

                            when (column) {
                                0 -> TableCell.Text(account.label)
                                1 -> TableCell.Text(agentName(account.agent), color = colors.textMuted)
                                2 -> TableCell.Text(modeName(account.agent, accountModeOf(current, account)) ?: "Same as ${agentName(account.agent)}", color = if (accountModeOf(current, account) == null) colors.textMuted else colors.text)

                                else -> TableCell.Slot {
                                    RowMenu(account.label) { close ->
                                        MenuItem("Change default mode", onClick = { close(); changingAccount = account }, leading = MyIcon.Pencil)
                                    }
                                }
                            }
                        },
                        rowDescription = { row -> "${accounts[row].label}, ${agentName(accounts[row].agent)}" },
                    )
                }
            }
        }

        onUsageLimit?.let { chosen ->
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("When a usage limit is reached", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Segmented(
                    UsageLimitChoices.map { it.first },
                    (UsageLimitChoices.firstOrNull { it.second == chosen } ?: UsageLimitChoices.first { it.second == "resume" }).first,
                    { label ->
                        val picked = UsageLimitChoices.first { it.first == label }.second
                        onUsageLimit = picked
                        store.act { store.api.saveUsageLimitSetting(UsageLimitSetting(picked)) }
                    },
                    groupLabel = "When a usage limit is reached",
                )
            }
        }

        settings?.let { current ->
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Label("Favourites, in order of preference", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
                    MyButton({ editingList = true }, text = "Edit list", variant = ButtonVariant.Outline)
                }

                Label("Any model starts on the first favourite with usage left, moves down as each reaches a limit, and moves back up at the next turn once usage returns. Star a model in any model menu to add it here.", style = Tokens.Type.Meta, color = colors.textMuted)

                if (current.anyModel.isEmpty()) {
                    Label("No favourites yet.", color = colors.textMuted)
                } else {
                    Table(
                        listOf(Col("Position", 32.dp, preferred = 40.dp), Col("Model", 140.dp, weight = 1f), Col("Harness", 100.dp, preferred = 160.dp)),
                        current.anyModel.size,
                        { row, column ->
                            val entry = current.anyModel[row]

                            when (column) {
                                0 -> TableCell.Text("${row + 1}", color = colors.textMuted)
                                1 -> TableCell.Text(entry.displayName(options, providers))
                                else -> TableCell.Text(entryHarness(entry), color = colors.textMuted)
                            }
                        },
                        header = false,
                        rowDescription = { row -> "Favourite ${row + 1}, ${current.anyModel[row].displayName(options, providers)}" },
                    )
                }
            }

            if (state.nodes.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Label("Any node", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                    Label("Any node starts a conversation on the node with the most free CPU and memory, weighted by these preferences.", style = Tokens.Type.Meta, color = colors.textMuted)

                    for (node in state.nodes.values.sortedBy { it.name.lowercase() }) {
                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                            Label(node.name, Modifier.width(DefaultModeNameWidth))
                            Segmented(
                                NodePreferenceChoices.map { it.second },
                                NodePreferenceChoices.first { it.first == (current.nodePreferences[node.id] ?: NodePreference.Normal) }.second,
                                { label ->
                                    val picked = NodePreferenceChoices.first { it.second == label }.first

                                    runner { settings = store.api.saveAgentSettings(SaveAgentSettingsRequest(current.defaultModes, current.anyModel, current.nodePreferences + (node.id to picked))) }
                                },
                                groupLabel = "Any node preference for ${node.name}",
                            )
                        }
                    }
                }
            }
        }
    }

    changingAccount?.let { account ->
        val current = settings

        if (current != null) {
            AccountModeDialog(
                account,
                accountModeOf(current, account),
                defaultModeOf(current, account.agent),
                onSave = { mode ->
                    store.api.setAccountMode(SetAccountModeRequest(account.agent, account.label, mode))
                    settings = store.api.agentSettings()
                },
                onDismiss = { changingAccount = null },
            )
        }
    }

    if (editingList) {
        settings?.let { current ->
            AnyModelDialog(
                current.anyModel,
                options,
                providers,
                onSave = { list -> settings = store.api.saveAgentSettings(SaveAgentSettingsRequest(current.defaultModes, list)) },
                onDismiss = { editingList = false },
            )
        }
    }
}

@Composable
private fun AccountModeDialog(account: AgentAccount, current: String?, agentDefault: String?, onSave: suspend (String?) -> Unit, onDismiss: () -> Unit) {
    var mode by remember(account.label) { mutableStateOf(current ?: SameAsAgent) }
    val runner = rememberAction(account.label)
    val agent = agentName(account.agent)
    val inherited = modeName(account.agent, agentDefault)?.let { "Same as $agent ($it)" } ?: "Same as $agent"
    val choices = listOf(Choice(SameAsAgent, inherited)) + fallbackModes(account.agent)

    Dialog(onDismiss) {
        Label("Default mode for ${account.label}", Modifier.semantics { heading() }, Tokens.Type.Title)
        OptionMenu("Mode", choices, mode, { mode = it }, fill = true)

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton({ runner { onSave(mode.ifEmpty { null }); onDismiss() } }, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
private fun AnyModelDialog(entries: List<ModelChoice>, options: List<ModelOption>, providers: List<ModelProvider>, onSave: suspend (List<ModelChoice>) -> Unit, onDismiss: () -> Unit) {
    var list by remember { mutableStateOf(entries) }
    val runner = rememberAction()

    Dialog(onDismiss) {
        Label("Favourites", Modifier.semantics { heading() }, Tokens.Type.Title)

        OrderedRows(
            names = list.mapIndexed { index, entry -> "${index + 1}  ${entry.displayName(options, providers)}" },
            onMove = { from, to -> list = list.toMutableList().also { moved -> moved.add(to, moved.removeAt(from)) } },
            onRemove = { index -> list = list.filterIndexed { position, _ -> position != index } },
        ) { index ->
            Label(entryHarness(list[index]), style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        }

        Row {
            ModelPicker(
                ModelPickerControls(
                    label = "Add a favourite",
                    options = options,
                    favourites = list,
                    providers = providers,
                    onPickModel = { picked -> list = list.withFavourite(picked, true) },
                    onFavourite = { picked, on -> list = list.withFavourite(picked, on) },
                ),
            )
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton({ runner { onSave(list); onDismiss() } }, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
fun rememberAgentSettings(store: Store): AgentSettings {
    var settings by remember { mutableStateOf(AgentSettings()) }

    LaunchedEffect(Unit) {
        store.agentSettings()?.let { settings = it }
    }

    return settings
}

class FavouriteModels(val list: List<ModelChoice>, val toggle: (ModelOption, Boolean) -> Unit)

@Composable
fun rememberFavouriteModels(store: Store): FavouriteModels {
    var settings by remember { mutableStateOf(AgentSettings()) }

    LaunchedEffect(Unit) {
        store.agentSettings()?.let { settings = it }
    }

    return FavouriteModels(settings.anyModel) { option, on ->
        val saved = settings.anyModel.withFavourite(option, on)

        settings = settings.copy(anyModel = saved)
        store.act { store.api.saveAgentSettings(SaveAgentSettingsRequest(settings.defaultModes, saved)) }
    }
}

private val NodePreferenceChoices = listOf(
    NodePreference.Preferred to "Preferred",
    NodePreference.Normal to "Normal",
    NodePreference.Avoid to "Avoid",
    NodePreference.Never to "Never",
)
