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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.ChatToolInfo
import mycelium.api.ChatToolServer
import mycelium.api.PatchChatToolServerRequest
import mycelium.state.ColumnAlign
import mycelium.state.Store

internal const val Overview = "Overview"
internal const val Usage = "Usage"

data class PageActions(
    val onBack: () -> Unit,
    val onAddAccount: () -> Unit,
    val onReplace: (ChatToolServer) -> Unit,
    val onRemoved: (ChatToolServer) -> Unit,
    val onReauthenticate: (ChatToolServer) -> Unit,
    val onOpenActivity: (Integration) -> Unit,
    val onOpenAccount: (ChatToolServer) -> Unit,
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

internal fun tokenLine(server: ChatToolServer): String = when {
    server.oauth -> "Signed in with OAuth"
    server.kind == "lastfm" && server.signedIn -> "Signed in to Last.fm"
    server.kind == "freeagent" && server.signedIn -> "Signed in to FreeAgent"
    server.tokenHint != null -> "Ends in ${server.tokenHint}"
    else -> "No token"
}

internal fun expiryFact(server: ChatToolServer): String {
    val at = server.tokenExpiresAt ?: return "No expiry set"
    val date = expiryDate(at) ?: return "No expiry set"
    val days = daysUntil(at) ?: return date

    return when {
        days < 0 -> "$date, expired"
        days == 0 -> "$date, today"
        days == 1 -> "$date, tomorrow"
        else -> "$date, in $days days"
    }
}

internal fun expiryLine(server: ChatToolServer): String = server.tokenExpiresAt?.let(::expiryDate)?.let { "Expires $it" } ?: "No expiry set"

@Composable
fun IntegrationPage(store: Store, integration: Integration, actions: PageActions, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    var tab by remember { mutableStateOf(Overview) }
    var removing by remember { mutableStateOf(false) }
    var recentConversations by remember { mutableStateOf<List<String>?>(null) }
    val live by store.state.collectAsState()
    val enable = integration.group == IntegrationGroup.Disabled

    LaunchedEffect(removing) {
        recentConversations = null

        if (removing) {
            store.act {
                val calls = integration.accounts.flatMap { store.api.integrationCalls(server = it.id) }
                recentConversations = calls.mapNotNull { it.conversationId }.distinct().map { live.conversations[it]?.title ?: "A conversation" }
            }
        }
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
            Section("Accounts") {
                for (account in integration.accounts) {
                    AccountLink(account) { actions.onOpenAccount(account) }
                }
            }

            Section("Tools") {
                val tools = integration.accounts.first().tools

                if (tools.isEmpty()) {
                    Label("No tools found.", color = colors.textMuted)
                }

                for (kind in listOf("read", "write")) {
                    ToolsTable(if (kind == "read") "Read" else "Write", tools.filter { (it.kind == "read") == (kind == "read") }, integration.accounts.first())
                }
            }

            Hairline()

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton(
                    {
                        for (account in integration.accounts) {
                            store.act { actions.onReplace(store.api.patchChatToolServer(account.id, PatchChatToolServerRequest(enabled = enable))) }
                        }
                    },
                    text = if (enable) "Enable" else "Disable",
                    variant = ButtonVariant.Outline,
                )
                MyButton({ removing = true }, text = "Remove ${integration.name}", variant = ButtonVariant.Danger)
            }
        }
    }

    if (removing) {
        val recent = recentConversations
        val used = when {
            recent == null -> ""
            recent.isEmpty() -> " No conversation used it in the last 30 days."
            else -> " Used in the last 30 days by: ${recent.joinToString(", ")}."
        }

        ConfirmDialog("Remove ${integration.name}?", "Removes ${integration.name}, its ${if (integration.accounts.size == 1) "account" else "${integration.accounts.size} accounts"} and their credentials.$used", "Remove", { removing = false }) {
            removing = false
            store.act {
                for (account in integration.accounts) {
                    store.api.deleteChatToolServer(account.id)
                    actions.onRemoved(account)
                }
            }
        }
    }
}

@Composable
private fun AccountLink(account: ChatToolServer, onOpen: () -> Unit) {
    val colors = MyTheme.colors

    Row(
        Modifier
            .fillMaxWidth()
            .pressable(onOpen, role = Role.Button, description = "Open ${accountTitle(account)}", shape = ControlShape)
            .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(accountTitle(account), Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 1)
        Label(expiryLine(account), style = Tokens.Type.Meta, color = if (accountState(account).attention) colors.statusAttention else colors.textMuted, maxLines = 1)
        IntegrationStatus(accountState(account), accountStatusWords(account))
        Icon(MyIcon.ChevronRight, colors.textMuted, Tokens.Size.IconSm)
    }
}

@Composable
internal fun Section(title: String, content: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label(title, Modifier.semantics { heading() }, Tokens.Type.MetaStrong, MyTheme.colors.textMuted)
        content()
    }
}

@Composable
internal fun ToolsTable(title: String, tools: List<ChatToolInfo>, server: ChatToolServer) {
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
                    TableCell.Text(if (tool.asks) "Asks first" else "Allowed", color = muted)
                }
            },
            lazy = false,
            rowKey = { tools[it].name },
        )
    }
}
