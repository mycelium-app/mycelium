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
import mycelium.api.ModelChoice
import mycelium.api.ModelProvider
import mycelium.state.ModelOption
import mycelium.state.PickerAccount
import mycelium.state.anyModelHint
import mycelium.state.favouriteOptions
import mycelium.state.isFavourite
import mycelium.state.matching

class PickerRefusal(val message: String, val rowId: String, val nodeId: String, val nodeName: String)

class ModelPickerControls(
    val label: String,
    val options: List<ModelOption>,
    val favourites: List<ModelChoice>,
    val providers: List<ModelProvider>,
    val onPickModel: (ModelOption) -> Unit,
    val onPickAny: (() -> Unit)? = null,
    val onFavourite: (ModelOption, Boolean) -> Unit,
    val chosen: ModelChoice? = null,
    val anyChosen: Boolean = false,
    val tooltip: String? = null,
    val accounts: List<PickerAccount> = emptyList(),
    val account: String? = null,
    val onPickAccount: ((String?, (PickerRefusal?) -> Unit) -> Unit)? = null,
    val onMove: (String) -> Unit = {},
    val focus: FocusRequester? = null,
    val problem: String? = null,
)

private const val ModelMenuTitle = "Model"

private const val NoNodeOnline = "No node online"

private const val ThisDevice = "This device"

private const val AnyAccount = "Any account"

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
private fun ModelRow(option: ModelOption, controls: ModelPickerControls, favourite: Boolean, showHarness: Boolean, onChoose: (ModelOption) -> Unit) {
    val chosen = controls.chosen?.let(option::isChosen) == true

    MenuItem(
        option.name,
        onClick = { onChoose(option) },
        checked = chosen,
        hint = option.where ?: option.harness.takeIf { showHarness },
        leadingContent = { Icon(option.agent?.let(::agentIcon) ?: MyIcon.Chat, if (option.available) MyTheme.colors.textMuted else MyTheme.colors.textFaint) },
        greyed = !option.available,
        description = "${option.harness}: ${option.name}${option.where?.let { ", ${it.replaceFirstChar(Char::lowercase)}" }.orEmpty()}",
        trailing = {
            MyButton(
                { controls.onFavourite(option, !favourite) },
                icon = MyIcon.Star,
                description = if (favourite) "Remove from favourites: ${option.name}" else "Add to favourites: ${option.name}",
                size = ButtonSize.Sm,
                selected = favourite,
                muted = !favourite,
            )
        },
    )
}

@Composable
private fun MenuHeading(text: String) {
    Hairline(Modifier.padding(vertical = Tokens.Space.S1))
    Label(text, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1).semantics { heading() }, Tokens.Type.MetaStrong)
}

@Composable
fun ModelPicker(controls: ModelPickerControls, maxWidth: Dp = Tokens.Size.AccountControl, trigger: (@Composable (open: () -> Unit, expanded: Boolean) -> Unit)? = null) {
    var open by remember { mutableStateOf(false) }

    Box {
        if (trigger != null) {
            trigger({ open = true }, open)
        } else {
            MyButton(
                { open = true },
                Modifier.then(controls.focus?.let { Modifier.focusRequester(it) } ?: Modifier).semantics { controls.problem?.let { error(it) } },
                text = controls.label,
                description = "$ModelMenuTitle: ${controls.label}",
                icon = if (controls.anyChosen) MyIcon.Layers else controls.chosen?.agent?.let(::agentIcon) ?: MyIcon.Chat,
                muted = true,
                trailingIcon = MyIcon.ChevronDown,
                selected = open,
                tooltip = controls.tooltip ?: ModelMenuTitle,
                maxWidth = maxWidth,
            )
        }

        if (open) {
            var query by remember { mutableStateOf("") }
            var choosingAccount by remember { mutableStateOf(false) }
            var refusal by remember { mutableStateOf<PickerRefusal?>(null) }
            val focus = remember { FocusRequester() }
            val touch = LocalTouch.current
            val favourites = controls.favourites.favouriteOptions(controls.options, controls.providers)
            val searching = query.isNotBlank()
            val found = controls.options.matching(query)
            val anyListed = controls.onPickAny != null && (!searching || "any model".contains(query.trim(), ignoreCase = true))

            LaunchedEffect(choosingAccount) {
                if (!touch && !choosingAccount) {
                    focus.requestFocus()
                }
            }

            fun choose(option: ModelOption) {
                if (option.available) {
                    controls.onPickModel(option)
                    open = false
                }
            }

            fun chooseAccount(rowId: String?) {
                controls.onPickAccount?.invoke(rowId) { problem -> if (problem == null) open = false else refusal = problem }
            }

            MenuPopup(onDismiss = { open = false }, width = Tokens.Size.Menu * 1.75f) {
                if (choosingAccount) {
                    Column(Modifier.semantics { contentDescription = "Account" }) {
                        MenuItem("Back to models", onClick = { choosingAccount = false; refusal = null }, leading = MyIcon.ChevronLeft)
                        MenuLabel("Runs on")
                        MenuItem(AnyAccount, onClick = { chooseAccount(null) }, checked = controls.account == null, hint = "Switches as usage runs out", leading = MyIcon.Person)

                        for (row in controls.accounts) {
                            val rowRefusal = refusal?.takeIf { it.rowId == row.id }

                            MenuItem(
                                row.label,
                                leading = agentIcon(row.agent),
                                onClick = { chooseAccount(row.id) },
                                checked = row.id == controls.account,
                                description = row.description(),
                                hintContent = { AccountDetail(row) },
                                checkSlot = true,
                            )

                            if (rowRefusal != null) {
                                ErrorBanner(
                                    rowRefusal.message,
                                    onDismiss = { refusal = null },
                                    Modifier.padding(vertical = Tokens.Space.S1),
                                    actions = {
                                        MyButton({ open = false; controls.onMove(rowRefusal.nodeId) }, text = "Move to ${rowRefusal.nodeName}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                                    },
                                )
                            }
                        }
                    }

                    return@MenuPopup
                }

                Column(Modifier.semantics { contentDescription = ModelMenuTitle }) {
                    TextInput(
                        query,
                        { query = it },
                        label = "Search models",
                        placeholder = "Search models",
                        showLabel = false,
                        leadingIcon = MyIcon.Search,
                        focusRequester = focus,
                        onSubmit = { found.firstOrNull { it.available }?.let(::choose) },
                    )
                    val scroll = rememberLazyListState()

                    LazyColumn(Modifier.heightIn(max = Tokens.Size.MenuList).scrollbar(scroll), state = scroll) {
                        if (anyListed) {
                            item(key = "any") {
                                MenuItem(
                                    "Any model",
                                    onClick = { controls.onPickAny?.invoke(); open = false },
                                    checked = controls.anyChosen,
                                    hint = anyModelHint(favourites),
                                    leading = MyIcon.Layers,
                                )
                            }
                        }

                        val starred = favourites.matching(query)

                        if (starred.isNotEmpty()) {
                            item(key = "favourites") { MenuHeading("Favourites") }
                            items(starred, key = { "favourite|${it.key}" }) { ModelRow(it, controls, favourite = true, showHarness = true, onChoose = ::choose) }
                        }

                        for ((harness, options) in found.sortedBy { it.section ?: "" }.groupBy { it.section ?: it.harness }) {
                            item(key = "heading|$harness") { MenuHeading(harness) }
                            items(options, key = { "model|${it.key}" }) { ModelRow(it, controls, favourite = controls.favourites.isFavourite(it), showHarness = false, onChoose = ::choose) }
                        }

                        if (found.isEmpty() && !anyListed && favourites.matching(query).isEmpty()) {
                            item { MenuLabel("No matching models") }
                        }

                        if (controls.onPickAccount != null && !searching) {
                            item(key = "account") {
                                Hairline(Modifier.padding(vertical = Tokens.Space.S1))
                                MenuItem(
                                    "Account",
                                    onClick = { choosingAccount = true },
                                    hint = controls.accounts.firstOrNull { it.id == controls.account }?.label ?: "Any signed-in account",
                                    leading = MyIcon.Person,
                                    trailing = { Icon(MyIcon.ChevronRight, MyTheme.colors.textMuted) },
                                )
                            }
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
    setup: (@Composable () -> Unit)? = null,
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
        setup?.invoke()

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
                    listOf(BarControl.Project, BarControl.Place, BarControl.Model).forEach { control(it) }
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
