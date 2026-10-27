package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.ChatToolServer

private const val AnyStatus = "Any status"
private const val AllCategories = "All"
private val CardMinWidth = 280.dp
private val MarkSize = 32.dp

@Composable
fun IntegrationMark(integration: Integration, size: androidx.compose.ui.unit.Dp = MarkSize) {
    val colors = MyTheme.colors
    val brand = integration.brand

    Box(Modifier.size(size).border(Tokens.Size.Hairline, colors.border, BlockShape), contentAlignment = Alignment.Center) {
        if (brand != null) {
            BrandLogo(brand, size / 2)
        } else {
            val icon = when (integration.key) {
                "email" -> MyIcon.Send
                "outline" -> MyIcon.File
                "caldav" -> MyIcon.Calendar
                "litellm" -> MyIcon.Cloud
                else -> MyIcon.Server
            }

            Icon(icon, colors.textMuted, size / 2)
        }
    }
}

@Composable
fun FilterChips(options: List<String>, selected: String, onSelect: (String) -> Unit, groupLabel: String) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        for (option in options) {
            MyButton({ onSelect(option) }, text = option, variant = ButtonVariant.Outline, size = ButtonSize.Sm, selected = option == selected, stateDescription = if (option == selected) "Selected" else "Not selected", description = "$groupLabel: $option")
        }
    }
}

@Composable
fun FilterMenu(label: String, options: List<String>, selected: String, onSelect: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }

    Box {
        MyButton({ open = true }, text = selected, description = "$label: $selected", variant = ButtonVariant.Outline, size = ButtonSize.Sm, trailingIcon = MyIcon.ChevronDown, selected = open)

        if (open) {
            MenuPopup(onDismiss = { open = false }) {
                for (option in options) {
                    MenuItem(option, onClick = {
                        open = false
                        onSelect(option)
                    }, checked = option == selected)
                }
            }
        }
    }
}

private fun toneOf(state: AccountState): Tone = when (state) {
    AccountState.Connected, AccountState.Disabled -> Tone.Muted
    AccountState.Error, AccountState.Expired -> Tone.Failed
    AccountState.NeedsSignIn, AccountState.ExpiresSoon -> Tone.Attention
}

private fun iconOf(state: AccountState): MyIcon = when (state) {
    AccountState.Connected -> MyIcon.Check
    AccountState.Disabled -> MyIcon.Minus
    AccountState.NeedsSignIn -> MyIcon.Key
    AccountState.ExpiresSoon -> MyIcon.Clock
    AccountState.Expired -> MyIcon.TriangleAlert
    AccountState.Error -> MyIcon.Alert
}

@Composable
fun IntegrationStatus(state: AccountState, words: String, modifier: Modifier = Modifier) {
    StatusMark(words, toneOf(state), iconOf(state), modifier)
}

data class IntegrationActions(
    val onOpen: (Integration) -> Unit,
    val onReauthenticate: (Integration) -> Unit,
    val onToggle: (Integration) -> Unit,
    val onRemove: (Integration) -> Unit,
)

@Composable
private fun IntegrationCard(integration: Integration, actions: IntegrationActions, modifier: Modifier) {
    val colors = MyTheme.colors
    val locked = integration.accounts.any { it.locked }
    val disabled = integration.group == IntegrationGroup.Disabled

    Column(
        modifier
            .border(Tokens.Size.Hairline, colors.border, BlockShape)
            .pressable({ actions.onOpen(integration) }, role = androidx.compose.ui.semantics.Role.Button, shape = BlockShape, base = colors.bgPanel)
            .padding(Tokens.Space.S3),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
            IntegrationMark(integration)

            Column(Modifier.weight(1f)) {
                Label(integration.name, style = Tokens.Type.UiStrong, maxLines = 1)
                Label(accountsSummary(integration), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            RowMenu(integration.name) { close ->
                MenuItem("Re-authenticate", onClick = {
                    close()
                    actions.onReauthenticate(integration)
                })
                MenuItem(if (disabled) "Enable" else "Disable", onClick = {
                    close()
                    actions.onToggle(integration)
                })
                MenuItem("Remove", onClick = {
                    close()
                    actions.onRemove(integration)
                })
            }
        }

        IntegrationStatus(integration.worst, statusWords(integration))
        Hairline()

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            if (locked) {
                Icon(MyIcon.Info, colors.textFaint, Tokens.Size.IconSm)
                Label("Asks every call, you review results", Modifier.weight(1f), Tokens.Type.Meta, colors.textFaint, maxLines = 1)
            } else if (integration.kind == "lastfm" && integration.accounts.none { it.signedIn }) {
                Label("Sign in to love tracks and scrobble", Modifier.weight(1f), Tokens.Type.Meta, colors.textFaint, maxLines = 1)
            } else {
                val count = integration.tools.size
                Label(if (count == 1) "1 tool" else "$count tools", style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
                val last = integration.lastUsedAt
                Label(if (last != null) "Last used ${timestamp(last, true)}" else "Not used yet", Modifier.weight(1f), Tokens.Type.Meta, colors.textFaint, maxLines = 1)
            }
        }
    }
}

@Composable
private fun CardGrid(items: List<Integration>, actions: IntegrationActions) {
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val columns = (maxWidth / CardMinWidth).toInt().coerceIn(1, 3)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            for (row in items.chunked(columns)) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    for (integration in row) {
                        IntegrationCard(integration, actions, Modifier.weight(1f))
                    }

                    repeat(columns - row.size) {
                        Spacer(Modifier.weight(1f))
                    }
                }
            }
        }
    }
}

@Composable
fun IntegrationsHome(servers: List<ChatToolServer>, actions: IntegrationActions, onAdd: () -> Unit, onActivity: () -> Unit) {
    val colors = MyTheme.colors
    var query by remember { mutableStateOf("") }
    var category by remember { mutableStateOf(AllCategories) }
    var status by remember { mutableStateOf(AnyStatus) }
    val everything = integrationsOf(servers)
    val shown = everything.filter { integration ->
        matchesSearch(integration, query) &&
            (category == AllCategories || integration.category.label == category) &&
            (status == AnyStatus || integration.group.title == status)
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label("Integrations", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong, maxLines = 1)
            MyButton(onActivity, text = "Activity", icon = MyIcon.Activity, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            MyButton(onAdd, text = "Add integration", icon = MyIcon.Plus, variant = ButtonVariant.Primary, size = ButtonSize.Sm)
        }

        Label("Connect services to agents and chats. New integrations are ready in every chat straight away.", style = Tokens.Type.Meta, color = colors.textMuted)

        if (everything.isEmpty()) {
            Column(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S6), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label("No integrations yet", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Connect mail, chat, documents or any MCP server so agents and chats can use them. Every call asks first unless you say otherwise.", style = Tokens.Type.Meta, color = colors.textMuted)
                Row { MyButton(onAdd, text = "Add your first integration", variant = ButtonVariant.Outline) }
            }

            return@Column
        }

        TextInput(query, { query = it }, label = "Search integrations and accounts", placeholder = "Search integrations and accounts", leadingIcon = MyIcon.Search, showLabel = false)
        FilterChips(listOf(AllCategories) + IntegrationCategory.entries.map { it.label }, category, { category = it }, "Category")
        FilterMenu("Status", listOf(AnyStatus) + IntegrationGroup.entries.map { it.title }, status, { status = it })

        if (shown.isEmpty()) {
            Label("No integrations match. Clear the search or the filters.", color = colors.textMuted)
        }

        for (group in IntegrationGroup.entries) {
            val inGroup = shown.filter { it.group == group }

            if (inGroup.isEmpty()) {
                continue
            }

            Label(group.title, Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)
            CardGrid(inGroup, actions)
        }
    }
}
