package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
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
import kotlinx.serialization.json.jsonPrimitive
import mycelium.api.CalendarAccess
import mycelium.api.ChatToolServer
import mycelium.api.IntegrationApiGrant
import mycelium.api.PatchChatToolServerRequest
import mycelium.state.Choice
import mycelium.state.Store

private const val Approvals = "Approvals"
private const val Availability = "Availability"
private const val AllGroups = "All groups"
private const val ChosenGroups = "Chosen groups"
private const val NoExpiry = "none"
private const val CustomDate = "custom"

private val ExpiryOptions = ExpiryPresets.map { Choice(it.toString(), "$it days") } + listOf(Choice(CustomDate, "Custom date"), Choice(NoExpiry, "No expiration"))

private fun grantAccess(grant: IntegrationApiGrant): String = if (grant.scopes.size > 1) "Read and write" else grant.scopes.firstOrNull()?.substringAfterLast(':')?.replaceFirstChar { it.uppercase() } ?: "Read"

private fun resolveExpiry(choice: String, customDate: String): String? = when (choice) {
    NoExpiry -> null
    CustomDate -> expiryOn(customDate)
    else -> expiryIn(choice.toInt())
}

@Composable
fun AccountPage(store: Store, integration: Integration, account: ChatToolServer, actions: PageActions, onBack: () -> Unit, onOpenConversation: (String) -> Unit) {
    val colors = MyTheme.colors
    val rename = rememberAction()
    val live by store.state.collectAsState()
    var tab by remember { mutableStateOf(Overview) }
    var grants by remember(account.id) { mutableStateOf<List<IntegrationApiGrant>>(emptyList()) }
    var changing by remember { mutableStateOf<String?>(null) }
    var removing by remember { mutableStateOf(false) }
    var renamed by remember { mutableStateOf<String?>(null) }
    val title = accountTitle(account)
    val reauthenticates = account.oauth || account.kind == "lastfm" || account.kind == "freeagent"
    val calendar = account.kind == "caldav"
    var calendars by remember(account.id) { mutableStateOf<List<CalendarAccess>?>(null) }
    var calendarsFailure by remember(account.id) { mutableStateOf<String?>(null) }

    LaunchedEffect(account.id, live.integrationsRevision) {
        store.refreshGroups()
        store.act { grants = store.api.integrationApiGrants(account.id) }
    }

    LaunchedEffect(account.id, calendar) {
        if (calendar) {
            calendarsFailure = attempt { calendars = store.api.integrationCalendars(account.id) }
        }
    }

    fun patch(request: PatchChatToolServerRequest) {
        store.act { actions.onReplace(store.api.patchChatToolServer(account.id, request)) }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        BackButton(integration.name, onBack, showText = true)

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
            IntegrationMark(integration, 44.dp)

            Column(Modifier.weight(1f)) {
                Label(title, Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
                Label(integration.name, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            IntegrationStatus(accountState(account), statusWords(integration.copy(accounts = listOf(account))))
        }

        TabRow(listOf(Overview, Approvals, Availability, Usage), tab, { tab = it }, "Account sections")

        when (tab) {
            Overview -> {
                Section("Credential") {
                    if (calendar) {
                        Fact("Username", account.account.orEmpty())
                    }

                    Fact(if (calendar) "App password" else "Token", tokenLine(account))
                    Fact("Expires", expiryFact(account))
                    Fact("Address", account.url)

                    val home = account.settings?.get("calendar_home")?.jsonPrimitive?.content

                    if (calendar && home != null) {
                        Fact("Calendar home", home)
                    }

                    if (account.kind == "lastfm" && !account.signedIn) {
                        Label("Not signed in: needed to love tracks and scrobble", color = colors.textMuted)
                    }

                    if (account.kind == "freeagent" && !account.signedIn) {
                        Label("Not signed in: needed to read anything in FreeAgent", color = colors.textMuted)
                    }

                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        if (reauthenticates) {
                            MyButton({ actions.onReauthenticate(account) }, text = "Re-authenticate", icon = MyIcon.Refresh, variant = ButtonVariant.Outline)
                        } else {
                            MyButton({ changing = "token" }, text = if (calendar) "Replace app password" else "Replace token", variant = ButtonVariant.Outline)
                            MyButton({ changing = "expiry" }, text = "Change expiry", variant = ButtonVariant.Outline)
                        }

                        if (account.kind == "lastfm" && account.signedIn) {
                            MyButton({
                                store.act {
                                    store.api.signOutOfLastFm(account.id)
                                    actions.onReplace(account.copy(signedIn = false))
                                }
                            }, text = "Sign out of Last.fm", variant = ButtonVariant.Outline)
                        }

                        if (account.kind == "freeagent" && account.signedIn) {
                            MyButton({
                                store.act {
                                    store.api.signOutOfFreeAgent(account.id)
                                    actions.onReplace(account.copy(signedIn = false))
                                }
                            }, text = "Sign out of FreeAgent", variant = ButtonVariant.Outline)
                        }
                    }
                }

                if (calendar) {
                    CalendarList(integration, calendars, calendarsFailure)
                }

                if (grants.isNotEmpty()) {
                    ApiAccessSection(integration.name, grants) { grant ->
                        store.act {
                            store.api.endSession(grant.tokenId)
                            grants = grants.filter { it.tokenId != grant.tokenId }
                        }
                    }
                }

                Hairline()

                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    MyButton({ renamed = account.name; rename.failure = null }, text = "Rename account", variant = ButtonVariant.Outline)

                    if (account.kind == "mcp" || account.kind == "litellm") {
                        MyButton({ store.act { actions.onReplace(store.api.refreshChatToolServer(account.id)) } }, text = "Refresh tools", icon = MyIcon.Refresh, variant = ButtonVariant.Outline)
                    }

                    MyButton({ patch(PatchChatToolServerRequest(enabled = !account.enabled)) }, text = if (account.enabled) "Disable account" else "Enable account", variant = ButtonVariant.Outline)
                    MyButton({ removing = true }, text = "Remove account", variant = ButtonVariant.Danger)
                }
            }

            Approvals -> ApprovalsSummary(account) { changing = "approvals" }

            Availability -> {
                Section("Available to") {
                    if (account.groups.isEmpty()) {
                        Label(AllGroups, color = colors.textMuted)
                    }

                    for (id in account.groups) {
                        Label(live.groups[id]?.name ?: "A group")
                    }
                }

                Row { MyButton({ changing = "availability" }, text = "Change availability", variant = ButtonVariant.Outline) }
            }

            else -> IntegrationUsage(store, integration.copy(accounts = listOf(account)), { actions.onOpenActivity(integration) }, onOpenConversation)
        }
    }

    when (changing) {
        "expiry" -> ChangeExpiryDialog(account, { changing = null }) { request ->
            changing = null
            patch(request)
        }

        "token" -> ReplaceTokenDialog(if (calendar) "app password" else "token", { changing = null }) { request ->
            changing = null
            patch(request)
        }

        "approvals" -> ApprovalsDialog(account, { changing = null }) { policy ->
            changing = null
            patch(PatchChatToolServerRequest(policy = policy))
        }

        "availability" -> ChangeAvailabilityDialog(store, account, { changing = null }) { groups ->
            changing = null
            store.act { actions.onReplace(store.api.setChatToolServerGroups(account.id, groups)) }
        }
    }

    renamed?.let { name ->
        RenameDialog("Rename account", "Account name", name, { renamed = it }, {
            rename {
                actions.onReplace(store.api.patchChatToolServer(account.id, PatchChatToolServerRequest(name = name.trim())))
                renamed = null
            }
        }, { renamed = null }, rename.failure, { rename.failure = null })
    }

    if (removing) {
        ConfirmDialog("Remove $title?", "Removes this ${integration.name} account and its credentials. Other accounts stay.", "Remove", { removing = false }) {
            removing = false
            store.act {
                store.api.deleteChatToolServer(account.id)
                actions.onRemoved(account)
            }
        }
    }
}

@Composable
private fun Fact(name: String, value: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label(name, Modifier.width(120.dp), Tokens.Type.Meta, MyTheme.colors.textMuted)
        Label(value, Modifier.weight(1f))
    }
}

@Composable
private fun CalendarList(integration: Integration, calendars: List<CalendarAccess>?, failure: String?) {
    val colors = MyTheme.colors
    val host = defaultIntegrationName(integration.accounts.first().url, integration.accounts.first().url)

    Section("Calendars") {
        Label("What agents can reach on $host. A calendar you add on the server appears here and to agents straight away.", style = Tokens.Type.Meta, color = colors.textMuted)

        when {
            failure != null -> Label(failure, style = Tokens.Type.Meta, color = colors.statusFailed)
            calendars == null -> Label("Loading calendars.", style = Tokens.Type.Meta, color = colors.textMuted)
            else -> Table(
                listOf(Col("Calendar", 120.dp, weight = 1f), Col("Access", 120.dp)),
                calendars.size,
                { row, column ->
                    if (column == 0) TableCell.Text(calendars[row].name) else TableCell.Text(if (calendars[row].readOnly) "Read only" else "Read and write", color = colors.textMuted)
                },
                lazy = false,
                rowKey = { calendars[it].name },
            )
        }
    }
}

@Composable
private fun ApiAccessSection(name: String, grants: List<IntegrationApiGrant>, onRevoke: (IntegrationApiGrant) -> Unit) {
    val colors = MyTheme.colors

    Section("API access") {
        Label("Agents that asked to call $name's API directly. Mycelium adds the token on the way through, so it never reaches the agent, and every call is in the Usage tab.", style = Tokens.Type.Meta, color = colors.textMuted)

        Table(
            listOf(Col("Conversation", 120.dp, weight = 1f), Col("Access", 120.dp), Col("Until", 96.dp, priority = 1), ActionColumn),
            grants.size,
            { row, column ->
                val grant = grants[row]

                when (column) {
                    0 -> TableCell.Text(grant.conversationTitle ?: "A conversation")
                    1 -> TableCell.Text(grantAccess(grant), color = colors.textMuted)
                    2 -> TableCell.Text(expiryDate(grant.expiresAt) ?: grant.expiresAt, color = colors.textMuted)
                    else -> TableCell.Slot {
                        MyButton({ onRevoke(grant) }, text = "Revoke", description = "Revoke access for ${grant.conversationTitle ?: "a conversation"}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                    }
                }
            },
            lazy = false,
            rowKey = { grants[it].tokenId },
        )
    }
}

@Composable
private fun ChangeExpiryDialog(account: ChatToolServer, onDismiss: () -> Unit, onSave: (PatchChatToolServerRequest) -> Unit) {
    val colors = MyTheme.colors
    var choice by remember { mutableStateOf(NoExpiry) }
    var customDate by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }

    fun save() {
        val expiresAt = resolveExpiry(choice, customDate)

        if (choice == CustomDate && expiresAt == null) {
            failure = "Enter the expiry date as year-month-day, for example 2027-01-24."

            return
        }

        onSave(if (expiresAt == null) PatchChatToolServerRequest(clearTokenExpiry = true) else PatchChatToolServerRequest(tokenExpiresAt = expiresAt))
    }

    Dialog(onDismiss) {
        Label("Change expiry", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label(expiryNote(account) ?: "This token has no expiry set.", style = Tokens.Type.Meta, color = colors.textMuted)

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
            OptionMenu("Expiration", ExpiryOptions, choice, { choice = it })
            resolveExpiry(choice, customDate)?.let(::expiryDate)?.let { Label("Expires $it.", style = Tokens.Type.Meta, color = colors.textMuted) }
        }

        if (choice == CustomDate) {
            TextInput(customDate, { customDate = it }, label = "Expiry date", placeholder = "2027-01-24", mono = true, error = failure, onSubmit = ::save)
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
private fun ReplaceTokenDialog(noun: String, onDismiss: () -> Unit, onSave: (PatchChatToolServerRequest) -> Unit) {
    val colors = MyTheme.colors
    var token by remember { mutableStateOf("") }
    var choice by remember { mutableStateOf(NoExpiry) }
    var customDate by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }

    fun save() {
        val expiresAt = resolveExpiry(choice, customDate)

        when {
            token.isBlank() -> failure = "Enter the new $noun."
            choice == CustomDate && expiresAt == null -> failure = "Enter the expiry date as year-month-day, for example 2027-01-24."
            else -> onSave(PatchChatToolServerRequest(bearerToken = token.trim(), tokenExpiresAt = expiresAt, clearTokenExpiry = if (expiresAt == null) true else null))
        }
    }

    Dialog(onDismiss) {
        Label("Replace $noun", Modifier.semantics { heading() }, Tokens.Type.Title)
        TextInput(token, { token = it; failure = null }, label = "New $noun", mono = true, masked = true, error = failure.takeIf { choice != CustomDate || token.isBlank() }, onSubmit = ::save)

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
            OptionMenu("Expiration", ExpiryOptions, choice, { choice = it })
            resolveExpiry(choice, customDate)?.let(::expiryDate)?.let { Label("Expires $it.", style = Tokens.Type.Meta, color = colors.textMuted) }
        }

        if (choice == CustomDate) {
            TextInput(customDate, { customDate = it }, label = "Expiry date", placeholder = "2027-01-24", mono = true, error = failure.takeIf { token.isNotBlank() }, onSubmit = ::save)
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
private fun ChangeAvailabilityDialog(store: Store, account: ChatToolServer, onDismiss: () -> Unit, onSave: (List<String>) -> Unit) {
    val colors = MyTheme.colors
    val live by store.state.collectAsState()
    var chosenMode by remember { mutableStateOf(account.groups.isNotEmpty()) }
    var chosen by remember { mutableStateOf(account.groups.toSet()) }
    val groups = live.groups.values.sortedBy { it.name.lowercase() }

    Dialog(onDismiss) {
        Label("Change availability", Modifier.semantics { heading() }, Tokens.Type.Title)
        Segmented(listOf(AllGroups, ChosenGroups), if (chosenMode) ChosenGroups else AllGroups, { chosenMode = it == ChosenGroups }, "Available to")

        if (chosenMode) {
            if (groups.isEmpty()) {
                Label("You have no groups yet. Create one in Settings, Groups.", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            for (group in groups) {
                val on = group.id in chosen

                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(on, "Available in ${group.name}", { chosen = if (on) chosen - group.id else chosen + group.id }, checkedState = "On", uncheckedState = "Off")
                    Label(group.name, maxLines = 1)
                }
            }

            Label("Choose at least one group. With none chosen, every group can use it.", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton({ onSave(if (chosenMode) chosen.toList() else emptyList()) }, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}
