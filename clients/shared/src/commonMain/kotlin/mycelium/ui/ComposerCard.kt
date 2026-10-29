package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.api.Conversation
import mycelium.api.ModelProvider
import mycelium.state.Choice
import mycelium.state.PickerAccount
import mycelium.state.RoutingGroup
import mycelium.state.agentName
import mycelium.state.Store

class PickerRefusal(val message: String, val rowId: String, val nodeId: String, val nodeName: String)

class AccountPickerControls(
    val title: String,
    val onPick: (String, (PickerRefusal?) -> Unit) -> Unit,
    val label: String? = null,
    val tooltip: String? = null,
    val accounts: List<PickerAccount> = emptyList(),
    val routing: List<Choice> = emptyList(),
    val current: String? = null,
    val providers: List<ModelProvider> = emptyList(),
    val providerId: String? = null,
    val model: String? = null,
    val recent: List<Pair<ModelProvider, String>> = emptyList(),
    val onFavourite: (ModelProvider, String, Boolean) -> Unit = { _, _, _ -> },
    val onPickModel: (ModelProvider, String) -> Unit = { _, _ -> },
    val onMove: (String) -> Unit = {},
    val focus: FocusRequester? = null,
    val problem: String? = null,
)

fun recentModels(conversations: Collection<Conversation>, providers: List<ModelProvider>, limit: Int = 5): List<Pair<ModelProvider, String>> {
    val picked = conversations.filter { it.providerId != null && it.model != null }.sortedByDescending { it.updatedAt }

    return picked
        .mapNotNull { conversation ->
            val provider = providers.firstOrNull { it.id == conversation.providerId } ?: return@mapNotNull null
            val model = conversation.model.orEmpty()

            if (model in provider.models && model !in provider.favourites) provider to model else null
        }
        .distinctBy { (provider, model) -> provider.id to model }
        .take(limit)
}

fun List<ModelProvider>.withFavourite(providerId: String, model: String, favourite: Boolean): List<ModelProvider> =
    map { if (it.id == providerId) it.copy(favourites = it.favourites.filter { name -> name != model } + listOfNotNull(model.takeIf { favourite })) else it }

fun Store.setFavouriteModel(change: ((List<ModelProvider>) -> List<ModelProvider>) -> Unit, provider: ModelProvider, model: String, favourite: Boolean) {
    change { it.withFavourite(provider.id, model, favourite) }

    act {
        val saved = api.setFavouriteModel(provider.id, model, favourite)

        change { current -> current.map { if (it.id == saved.id) saved.copy(favourites = it.favourites) else it } }
    }
}

private sealed interface PickerEntry {
    val key: String

    data class Heading(val text: String, val divided: Boolean) : PickerEntry {
        override val key = "heading|$text"
    }

    data class Provider(val text: String) : PickerEntry {
        override val key = "provider|$text"
    }

    data class Routing(val choice: Choice) : PickerEntry {
        override val key = "routing|${choice.id}"
    }

    data class Account(val row: PickerAccount) : PickerEntry {
        override val key = "account|${row.id}"
    }

    data class Refused(val refusal: PickerRefusal) : PickerEntry {
        override val key = "refused|${refusal.rowId}"
    }

    data class Model(val section: String, val provider: ModelProvider, val model: String, val showProvider: Boolean) : PickerEntry {
        override val key = "$section|${provider.id}|$model"
    }
}

private const val ChatModelsHeading = "Chat models"

private const val NoNodeOnline = "No node online"

private const val ThisDevice = "This device"

private fun PickerAccount.description(): String = when {
    signIn -> "$label, $usage"
    node == null -> "$label, $NoNodeOnline"
    else -> "$label on ${if (here) ThisDevice.lowercase() else node.name}, $usage"
}

@Composable
private fun AccountDetail(row: PickerAccount) {
    val muted = MyTheme.colors.textMuted
    val node = row.node

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
        if (node == null && !row.signIn) {
            Label(NoNodeOnline, style = Tokens.Type.Meta, color = muted, maxLines = 1)

            return@Row
        }

        if (node != null && !row.signIn) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Icon(nodeIcon(node), muted, Tokens.Size.IconSm)
                Label(if (row.here) ThisDevice else node.name, style = Tokens.Type.Meta, color = muted, maxLines = 1)
            }
        }

        Label(row.usage, style = Tokens.Type.Meta, color = muted, maxLines = 1)
    }
}

@Composable
fun AccountPicker(controls: AccountPickerControls, maxWidth: Dp = Tokens.Size.AccountControl) {
    val current = controls.providers.firstOrNull { it.id == controls.providerId }
    val label = controls.label ?: if (current != null && controls.model != null) "${current.name} · ${controls.model}" else "Choose a model"
    var open by remember { mutableStateOf(false) }

    Box {
        MyButton(
            { open = true },
            Modifier.then(controls.focus?.let { Modifier.focusRequester(it) } ?: Modifier).semantics { controls.problem?.let { error(it) } },
            text = label,
            description = "Account: $label",
            icon = MyIcon.Person,
            muted = true,
            trailingIcon = MyIcon.ChevronDown,
            selected = open,
            tooltip = controls.tooltip ?: "Account",
            maxWidth = maxWidth,
        )

        if (open) {
            var query by remember { mutableStateOf("") }
            var refusal by remember { mutableStateOf<PickerRefusal?>(null) }
            val searching = query.isNotBlank()
            val terms = query.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
            val listed = controls.providers.filter { it.models.isNotEmpty() }

            fun matches(vararg names: String?) = terms.all { term -> names.any { it?.contains(term, ignoreCase = true) == true } }

            val listing = buildList<PickerEntry> {
                fun section(heading: String, rows: List<PickerEntry>) {
                    if (rows.isEmpty()) {
                        return
                    }

                    add(PickerEntry.Heading(heading, isNotEmpty()))
                    addAll(rows)
                }

                if (!searching) {
                    val favourites = listed.flatMap { provider -> provider.favourites.filter { it in provider.models }.map { provider to it } }

                    section("Favourites", favourites.map { (provider, model) -> PickerEntry.Model("favourite", provider, model, true) })
                    section("Recent", controls.recent.map { (provider, model) -> PickerEntry.Model("recent", provider, model, true) })
                }

                section(RoutingGroup, controls.routing.filter { matches(it.name, it.description) }.map { PickerEntry.Routing(it) })

                val harnesses = (listOf("claude", "codex") + controls.accounts.map { it.agent }).distinct()

                for (agent in harnesses) {
                    val rows = controls.accounts
                        .filter { it.agent == agent && matches(agentName(agent), agent, it.account, it.node?.name) }
                        .sortedByDescending { it.here }

                    section(agentName(agent), rows.map { PickerEntry.Account(it) })
                }

                val chatModels = buildList<PickerEntry> {
                    for (provider in listed) {
                        val matching = provider.models.filter { model -> matches(provider.name, model) }

                        for (group in matching.groupBy { if ('/' in it) it.substringBefore('/') else "" }.entries.sortedBy { it.key }) {
                            add(PickerEntry.Provider(if (group.key.isEmpty()) provider.name else "${provider.name} · ${group.key}"))
                            group.value.forEach { add(PickerEntry.Model("provider", provider, it, false)) }
                        }
                    }
                }

                section(ChatModelsHeading, chatModels)
            }
            val entries = refusal?.let { refused ->
                val at = listing.indexOfFirst { it is PickerEntry.Account && it.row.id == refused.rowId }

                if (at < 0) listing else listing.take(at + 1) + PickerEntry.Refused(refused) + listing.drop(at + 1)
            } ?: listing
            val focus = remember { FocusRequester() }
            val touch = LocalTouch.current

            LaunchedEffect(Unit) {
                if (!touch) {
                    focus.requestFocus()
                }
            }

            fun choose(entry: PickerEntry) {
                when (entry) {
                    is PickerEntry.Model -> {
                        controls.onPickModel(entry.provider, entry.model)
                        open = false
                    }
                    is PickerEntry.Account -> controls.onPick(entry.row.id) { problem -> if (problem == null) open = false else refusal = problem }
                    is PickerEntry.Routing -> controls.onPick(entry.choice.id) { problem -> if (problem == null) open = false else refusal = problem }
                    else -> Unit
                }
            }

            MenuPopup(onDismiss = { open = false }, width = Tokens.Size.Menu * 1.75f) {
                Column(Modifier.semantics { contentDescription = controls.title }) {
                    TextInput(
                        query,
                        { query = it; refusal = null },
                        label = "Search accounts, nodes and models",
                        placeholder = "Account, node or model",
                        showLabel = false,
                        leadingIcon = MyIcon.Search,
                        focusRequester = focus,
                        onSubmit = { entries.firstOrNull { it is PickerEntry.Account || it is PickerEntry.Routing || it is PickerEntry.Model }?.let(::choose) },
                    )
                    val scroll = rememberLazyListState()

                    LazyColumn(Modifier.heightIn(max = Tokens.Size.MenuList).scrollbar(scroll), state = scroll) {
                        items(entries, key = { it.key }) { entry ->
                            when (entry) {
                                is PickerEntry.Heading -> {
                                    if (entry.divided) {
                                        Hairline(Modifier.padding(vertical = Tokens.Space.S1))
                                    }

                                    Label(entry.text, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1).semantics { heading() }, Tokens.Type.MetaStrong)
                                }
                                is PickerEntry.Provider -> MenuLabel(entry.text)
                                is PickerEntry.Refused -> ErrorBanner(
                                    entry.refusal.message,
                                    onDismiss = { refusal = null },
                                    Modifier.padding(vertical = Tokens.Space.S1),
                                    actions = {
                                        MyButton({ open = false; controls.onMove(entry.refusal.nodeId) }, text = "Move to ${entry.refusal.nodeName}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                                    },
                                )
                                is PickerEntry.Routing -> MenuItem(entry.choice.name, onClick = { choose(entry) }, checked = entry.choice.id == controls.current, hint = entry.choice.description)
                                is PickerEntry.Account -> MenuItem(
                                    entry.row.label,
                                    onClick = { choose(entry) },
                                    checked = entry.row.id == controls.current,
                                    description = entry.row.description(),
                                    hintContent = { AccountDetail(entry.row) },
                                    checkSlot = true,
                                )
                                is PickerEntry.Model -> {
                                    val provider = entry.provider
                                    val model = entry.model
                                    val favourite = model in provider.favourites

                                    MenuItem(
                                        model,
                                        onClick = { choose(entry) },
                                        checked = provider.id == controls.providerId && model == controls.model,
                                        hint = if (entry.showProvider) provider.name else null,
                                        trailing = {
                                            MyButton(
                                                { controls.onFavourite(provider, model, !favourite) },
                                                icon = MyIcon.Star,
                                                description = if (favourite) "Remove $model from favourites" else "Add $model to favourites",
                                                size = ButtonSize.Sm,
                                                selected = favourite,
                                            )
                                        },
                                    )
                                }
                            }
                        }

                        if (entries.isEmpty()) {
                            item { MenuLabel("No matching accounts, nodes or models") }
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun ComposerCard(
    value: TextFieldValue,
    onValueChange: (TextFieldValue) -> Unit,
    placeholder: String,
    onSubmit: () -> Unit,
    attachments: AttachmentControls,
    kind: ConversationKind,
    modifier: Modifier = Modifier,
    error: String? = null,
    assistantActions: AssistantActionControls? = null,
    project: (@Composable () -> Unit)? = null,
    place: (@Composable () -> Unit)? = null,
    notice: (@Composable () -> Unit)? = null,
    account: (@Composable () -> Unit)? = null,
    model: (@Composable () -> Unit)? = null,
) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val shape = RoundedCornerShape(Tokens.Radius.Card)
    val source = remember { MutableInteractionSource() }

    DisposableEffect(platform, attachments.onAdd) {
        platform?.listenForFiles(attachments.onAdd)

        onDispose { platform?.listenForFiles(null) }
    }

    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Column(
            Modifier
                .fillMaxWidth()
                .background(colors.bgRaised, shape)
                .border(Tokens.Size.Hairline, colors.border, shape)
                .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2),
        ) {
            assistantActions?.let { ActionChips(it) }
            AttachmentChips(attachments)

            MessageField(
                text = value,
                onTextChange = onValueChange,
                source = source,
                onSubmit = onSubmit,
                onPaste = {
                    val pasted = platform?.clipboardFiles().orEmpty()

                    attachments.onAdd(pasted)
                    pasted.isNotEmpty()
                },
                onKey = { false },
                onOpenActions = assistantActions?.let { { it.menu.open = true } },
                modifier = Modifier.heightIn(min = 88.dp).padding(top = Tokens.Space.S1),
                alignment = Alignment.TopStart,
                placeholder = placeholder,
            )

            notice?.invoke()

            val control: @Composable (BarControl) -> Unit = { which ->
                when (which) {
                    BarControl.Attach -> MyButton({ platform?.pickFiles(attachments.onAdd) }, icon = MyIcon.Paperclip, description = "Attach files", muted = true)
                    BarControl.Project -> project?.invoke()
                    BarControl.Place -> place?.invoke()
                    BarControl.Actions -> assistantActions?.let { ActionsButton(it) }
                    BarControl.Account -> account?.invoke()
                    BarControl.Model -> model?.invoke()
                    BarControl.Send -> MyButton(onSubmit, icon = MyIcon.Send, description = "Send", muted = true)
                    else -> Unit
                }
            }

            if (LocalTouch.current && kind == ConversationKind.NewConversation) {
                Row(
                    Modifier.fillMaxWidth().horizontalScrollFaded().semantics { contentDescription = "Start options" },
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    listOf(BarControl.Project, BarControl.Place, BarControl.Account, BarControl.Model).forEach { control(it) }
                }

                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
                    control(BarControl.Attach)
                    control(BarControl.Send)
                }
            } else {
                ComposerBar(barControls(kind, HarnessReport(models = model != null)), render = control)
            }
        }

        if (error != null) {
            Row(Modifier.semantics { liveRegion = LiveRegionMode.Assertive }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm)
                Label(error, style = Tokens.Type.Meta, color = colors.statusFailed)
            }
        }
    }
}
