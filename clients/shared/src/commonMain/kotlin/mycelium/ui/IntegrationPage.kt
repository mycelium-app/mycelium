package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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
import mycelium.api.ChatToolInfo
import mycelium.api.ChatToolServer
import mycelium.api.IntegrationPolicy
import mycelium.api.PatchChatToolServerRequest
import mycelium.state.ColumnAlign
import mycelium.state.Store

private const val Overview = "Overview"
private const val Usage = "Usage"
private const val AllGroups = "All groups"
private const val ChosenGroups = "Chosen groups"

data class PageActions(
    val onBack: () -> Unit,
    val onAddAccount: () -> Unit,
    val onReplace: (ChatToolServer) -> Unit,
    val onRemoved: (ChatToolServer) -> Unit,
    val onReauthenticate: (ChatToolServer) -> Unit,
    val onOpenActivity: (Integration) -> Unit,
)

private fun kindLine(integration: Integration): String {
    val first = integration.accounts.first()
    val origin = when {
        first.forgeId != null -> "From your forge"
        entryFor(integration) != null -> "Built in"
        else -> "MCP server"
    }

    return "$origin · ${integration.category.label}"
}

private fun tokenLine(server: ChatToolServer): String = when {
    server.oauth -> "Signed in with OAuth"
    server.kind == "lastfm" && server.signedIn -> "Signed in to Last.fm"
    server.tokenHint != null -> "Ends in ${server.tokenHint}"
    else -> "No token"
}

private fun expiryLine(server: ChatToolServer): String = server.tokenExpiresAt?.let(::expiryDate)?.let { "Expires $it" } ?: "No expiry set"

@Composable
fun IntegrationPage(store: Store, integration: Integration, actions: PageActions, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    var tab by remember { mutableStateOf(Overview) }
    var selectedId by remember(integration.key) { mutableStateOf<String?>(null) }
    var renaming by remember { mutableStateOf<ChatToolServer?>(null) }
    var renamed by remember { mutableStateOf("") }
    var renameFailure by remember { mutableStateOf<String?>(null) }
    var removing by remember { mutableStateOf<ChatToolServer?>(null) }
    var replacingId by remember { mutableStateOf<String?>(null) }
    var recentConversations by remember { mutableStateOf<List<String>?>(null) }
    val attempt = rememberAttempt { renameFailure = it }
    val live by store.state.collectAsState()
    val selected = integration.accounts.find { it.id == selectedId } ?: integration.accounts.first()

    LaunchedEffect(removing) {
        recentConversations = null

        removing?.let { target ->
            store.act {
                val calls = store.api.integrationCalls(server = target.id)
                recentConversations = calls.mapNotNull { it.conversationId }.distinct().map { live.conversations[it]?.title ?: "A conversation" }
            }
        }
    }

    fun reauthenticate(target: ChatToolServer) {
        selectedId = target.id

        if (target.oauth || target.kind == "lastfm") {
            actions.onReauthenticate(target)
        } else {
            replacingId = target.id
        }
    }

    fun toggle(target: ChatToolServer) {
        store.act { actions.onReplace(store.api.patchChatToolServer(target.id, PatchChatToolServerRequest(enabled = !target.enabled))) }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        BackButton("Integrations", actions.onBack, showText = true)

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
            IntegrationMark(integration, 44.dp)

            Column(Modifier.weight(1f)) {
                Label(integration.name, Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
                Label(kindLine(integration), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            IntegrationStatus(integration.worst, statusWords(integration))
            MyButton(actions.onAddAccount, text = "Add account", icon = MyIcon.Plus, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        TabRow(listOf(Overview, Usage), tab, { tab = it }, "Integration sections")

        if (tab == Usage) {
            IntegrationUsage(store, integration, { actions.onOpenActivity(integration) }, onOpenConversation)
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label("Accounts", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                Table(
                    listOf(
                        Col("Account", 120.dp, weight = 1f),
                        Col("Status", 120.dp),
                        Col("Credential", 96.dp, max = 200.dp, priority = 1),
                        Col("Expiry", 96.dp, priority = 1),
                        ActionColumn,
                    ),
                    integration.accounts.size,
                    { row, column ->
                        val account = integration.accounts[row]

                        when (column) {
                            0 -> TableCell.Text(accountTitle(account), if (account.id == selected.id) Tokens.Type.UiStrong else Tokens.Type.Ui)
                            1 -> TableCell.Slot(accountStatusWords(account), Tokens.Size.IconSm + Tokens.Space.S1) {
                                IntegrationStatus(accountState(account), accountStatusWords(account))
                            }
                            2 -> TableCell.Text(tokenLine(account), color = colors.textMuted)
                            3 -> TableCell.Text(expiryLine(account), color = if (accountState(account).attention) colors.statusAttention else colors.textMuted)
                            else -> TableCell.Slot {
                                RowMenu(accountTitle(account)) { close ->
                                    MenuItem("Re-authenticate", onClick = {
                                        close()
                                        reauthenticate(account)
                                    })
                                    if (account.kind == "lastfm") {
                                        if (account.signedIn) {
                                            MenuItem("Sign out of Last.fm", onClick = {
                                                close()
                                                store.act {
                                                    store.api.signOutOfLastFm(account.id)
                                                    actions.onReplace(account.copy(signedIn = false))
                                                }
                                            })
                                        } else {
                                            MenuItem("Sign in to Last.fm", onClick = {
                                                close()
                                                actions.onReauthenticate(account)
                                            })
                                        }
                                    }

                                    if (account.kind == "mcp" || account.kind == "litellm") {
                                        MenuItem("Refresh tools", onClick = {
                                            close()
                                            store.act { actions.onReplace(store.api.refreshChatToolServer(account.id)) }
                                        })
                                    }

                                    MenuItem("Rename", onClick = {
                                        close()
                                        renamed = account.name
                                        renameFailure = null
                                        renaming = account
                                    })
                                    MenuItem(if (account.enabled) "Disable" else "Enable", onClick = {
                                        close()
                                        toggle(account)
                                    })
                                    MenuItem("Remove", onClick = {
                                        close()
                                        removing = account
                                    })
                                }
                            }
                        }
                    },
                    rowKey = { integration.accounts[it].id },
                    onRow = { selectedId = integration.accounts[it].id },
                    rowDescription = { "Settings for ${accountTitle(integration.accounts[it])}" },
                )
            }

            AccountSettings(store, selected, actions, replacingId == selected.id) { replacingId = null }

            Hairline()

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({ reauthenticate(selected) }, text = "Re-authenticate", icon = MyIcon.Refresh, variant = ButtonVariant.Outline)
                MyButton({ toggle(selected) }, text = if (selected.enabled) "Disable" else "Enable", variant = ButtonVariant.Outline)
                MyButton({ removing = selected }, text = "Remove ${integration.name}", variant = ButtonVariant.Danger)
            }
        }
    }

    renaming?.let { target ->
        RenameDialog("Rename account", "Account name", renamed, { renamed = it }, {
            attempt {
                actions.onReplace(store.api.patchChatToolServer(target.id, PatchChatToolServerRequest(name = renamed.trim())))
                renaming = null
            }
        }, { renaming = null }, renameFailure, { renameFailure = null })
    }

    removing?.let { target ->
        val recent = recentConversations
        val used = when {
            recent == null -> ""
            recent.isEmpty() -> " No conversation used it in the last 30 days."
            else -> " Used in the last 30 days by: ${recent.joinToString(", ")}."
        }
        val others = integration.accounts.size - 1
        val scope = if (others > 0) "Removes the ${accountTitle(target)} account of ${integration.name} and its credentials." else "Removes ${integration.name} and its credentials."

        ConfirmDialog("Remove ${integration.name}?", "$scope$used", "Remove", { removing = null }) {
            removing = null
            store.act {
                store.api.deleteChatToolServer(target.id)
                actions.onRemoved(target)
            }
        }
    }
}

@Composable
private fun AccountSettings(store: Store, server: ChatToolServer, actions: PageActions, replacing: Boolean, onReplaced: () -> Unit) {
    val colors = MyTheme.colors
    val live by store.state.collectAsState()
    var customDate by remember(server.id) { mutableStateOf("") }
    var dateFailure by remember(server.id) { mutableStateOf<String?>(null) }
    var newToken by remember(server.id) { mutableStateOf("") }
    var chosenMode by remember(server.id) { mutableStateOf(server.groups.isNotEmpty()) }

    LaunchedEffect(Unit) {
        store.refreshGroups()
    }

    fun patch(request: PatchChatToolServerRequest) {
        store.act { actions.onReplace(store.api.patchChatToolServer(server.id, request)) }
    }

    fun setGroups(groups: List<String>) {
        store.act { actions.onReplace(store.api.setChatToolServerGroups(server.id, groups)) }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Label("Settings for ${accountTitle(server)}", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        Section("Asking and review") {
            IntegrationPolicyEditor(server) { policy: IntegrationPolicy -> patch(PatchChatToolServerRequest(policy = policy)) }
        }

        Section("Token and expiry") {
            Label(tokenLine(server), color = colors.textMuted)

            if (server.kind == "lastfm" && !server.signedIn) {
                Label("Not signed in: needed to love tracks and scrobble", color = colors.textMuted)
            }

            val state = accountState(server)

            if (state == AccountState.Expired) {
                val date = server.tokenExpiresAt?.let(::expiryDate)

                Label("Token expired${date?.let { " $it" }.orEmpty()}. Agents cannot use this account until you re-authenticate it.", color = colors.statusFailed)
            } else if (state == AccountState.ExpiresSoon) {
                val days = server.tokenExpiresAt?.let { daysUntil(it) }
                val date = server.tokenExpiresAt?.let(::expiryDate).orEmpty()

                Label(if (days != null) "Expires in $days days, on $date. Replace the token before then." else "Expires $date. Replace the token before then.", color = colors.statusAttention)
            } else {
                expiryNote(server)?.let { Label(it, color = colors.textMuted) } ?: Label("No expiry set.", color = colors.textMuted)
            }

            Label("Set expiry", style = Tokens.Type.MetaStrong, color = colors.textMuted)

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                for (days in ExpiryPresets) {
                    MyButton({ patch(PatchChatToolServerRequest(tokenExpiresAt = expiryIn(days))) }, text = "$days days", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }

                MyButton({ patch(PatchChatToolServerRequest(clearTokenExpiry = true)) }, text = "No expiry", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }

            fun setDate() {
                val iso = expiryOn(customDate)

                if (iso == null) {
                    dateFailure = "Enter the expiry date as year-month-day, for example 2027-01-24."
                } else {
                    dateFailure = null
                    patch(PatchChatToolServerRequest(tokenExpiresAt = iso))
                }
            }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
                TextInput(customDate, { customDate = it }, label = "Custom expiry date", placeholder = "2027-01-24", mono = true, error = dateFailure, onSubmit = ::setDate, modifier = Modifier.weight(1f))
                MyButton(::setDate, text = "Set date", variant = ButtonVariant.Outline)
            }

            if (replacing || state == AccountState.Expired) {
                fun replace() {
                    val token = newToken.trim()

                    if (token.isNotEmpty()) {
                        newToken = ""
                        onReplaced()
                        patch(PatchChatToolServerRequest(bearerToken = token, clearTokenExpiry = true))
                    }
                }

                if (!server.oauth && server.kind != "lastfm") {
                    TextInput(newToken, { newToken = it }, label = "New token", mono = true, masked = true, onSubmit = ::replace)
                    Row { MyButton(::replace, text = "Replace token", variant = ButtonVariant.Outline) }
                }
            }
        }

        Section("Available to") {
            Segmented(listOf(AllGroups, ChosenGroups), if (chosenMode) ChosenGroups else AllGroups, { option ->
                chosenMode = option == ChosenGroups

                if (!chosenMode && server.groups.isNotEmpty()) {
                    setGroups(emptyList())
                }
            }, "Available to")

            if (chosenMode) {
                val groups = live.groups.values.sortedBy { it.name.lowercase() }

                if (groups.isEmpty()) {
                    Label("You have no groups yet. Create one in Settings, Groups.", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                for (group in groups) {
                    val on = group.id in server.groups

                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(on, "Available in ${group.name}", { setGroups(if (on) server.groups - group.id else server.groups + group.id) }, checkedState = "On", uncheckedState = "Off")
                        Label(group.name, maxLines = 1)
                    }
                }

                if (server.groups.isEmpty() && groups.isNotEmpty()) {
                    Label("Choose at least one group. With none chosen, every group can use it.", style = Tokens.Type.Meta, color = colors.textMuted)
                }
            }
        }

        Section("Tools") {
            if (server.tools.isEmpty()) {
                Label("No tools found.", color = colors.textMuted)
            }

            for (kind in listOf("read", "write")) {
                ToolsTable(if (kind == "read") "Read" else "Write", server.tools.filter { (it.kind == "read") == (kind == "read") }, server)
            }
        }
    }
}

@Composable
private fun Section(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label(title, Modifier.semantics { heading() }, Tokens.Type.MetaStrong, MyTheme.colors.textMuted)
        content()
    }
}

@Composable
private fun ToolsTable(title: String, tools: List<ChatToolInfo>, server: ChatToolServer) {
    val muted = MyTheme.colors.textMuted

    if (tools.isEmpty()) {
        return
    }

    Column(Modifier.fillMaxWidth().padding(bottom = Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Table(
            listOf(Col("$title tools", 120.dp, weight = 1f), Col("Asks", 72.dp, align = ColumnAlign.End)),
            tools.size,
            { row, column ->
                val tool = tools[row]

                if (column == 0) {
                    TableCell.Text(tool.name, Tokens.Type.Code)
                } else {
                    TableCell.Text(if (tool.asks || server.locked) "Asks first" else "Allowed", color = muted)
                }
            },
            lazy = false,
            rowKey = { tools[it].name },
        )
    }
}
