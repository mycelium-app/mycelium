package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
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
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import mycelium.api.Conversation
import mycelium.api.ModelProvider
import mycelium.state.Store

class ModelPickerControls(
    val providers: List<ModelProvider>,
    val providerId: String?,
    val model: String?,
    val recent: List<Pair<ModelProvider, String>> = emptyList(),
    val onFavourite: (ModelProvider, String, Boolean) -> Unit = { _, _, _ -> },
    val onSelect: (ModelProvider, String) -> Unit,
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

private sealed interface ModelMenuEntry {
    val key: String

    data class Heading(val text: String) : ModelMenuEntry {
        override val key = "heading|$text"
    }

    data class Choice(val section: String, val provider: ModelProvider, val model: String, val showProvider: Boolean) : ModelMenuEntry {
        override val key = "$section|${provider.id}|$model"
    }
}

@Composable
fun ModelPicker(controls: ModelPickerControls) {
    val current = controls.providers.firstOrNull { it.id == controls.providerId }
    val label = if (current != null && controls.model != null) "${current.name} · ${controls.model}" else "Choose a model"
    var open by remember { mutableStateOf(false) }

    Box {
        MyButton({ open = true }, text = label, description = "Model: $label", muted = true, trailingIcon = MyIcon.ChevronDown, selected = open, tooltip = "Model")

        if (open) {
            var query by remember { mutableStateOf("") }
            val searching = query.isNotBlank()
            val terms = query.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
            val listed = controls.providers.filter { it.models.isNotEmpty() }
            val entries = buildList {
                if (!searching) {
                    val favourites = listed.flatMap { provider -> provider.favourites.filter { it in provider.models }.map { provider to it } }

                    if (favourites.isNotEmpty()) {
                        add(ModelMenuEntry.Heading("Favourites"))
                        favourites.forEach { (provider, model) -> add(ModelMenuEntry.Choice("favourite", provider, model, true)) }
                    }

                    if (controls.recent.isNotEmpty()) {
                        add(ModelMenuEntry.Heading("Recent"))
                        controls.recent.forEach { (provider, model) -> add(ModelMenuEntry.Choice("recent", provider, model, true)) }
                    }
                }

                for (provider in listed) {
                    val matching = provider.models.filter { model -> terms.all { provider.name.contains(it, ignoreCase = true) || model.contains(it, ignoreCase = true) } }
                    val groups = matching.groupBy { if ('/' in it) it.substringBefore('/') else "" }.entries.sortedBy { it.key }

                    for (group in groups) {
                        add(ModelMenuEntry.Heading(if (group.key.isEmpty()) provider.name else "${provider.name} · ${group.key}"))
                        group.value.forEach { add(ModelMenuEntry.Choice("provider", provider, it, false)) }
                    }
                }
            }
            val firstChoice = entries.filterIsInstance<ModelMenuEntry.Choice>().firstOrNull()
            val focus = remember { FocusRequester() }
            val touch = LocalTouch.current

            LaunchedEffect(Unit) {
                if (!touch) {
                    focus.requestFocus()
                }
            }

            fun choose(provider: ModelProvider, model: String) {
                controls.onSelect(provider, model)
                open = false
            }

            MenuPopup(onDismiss = { open = false }, width = Tokens.Size.Menu * 1.75f) {
                TextInput(query, { query = it }, label = "Search models", placeholder = "Model or provider", showLabel = false, modifier = Modifier.focusRequester(focus), onSubmit = { firstChoice?.let { choose(it.provider, it.model) } })
                val scroll = rememberLazyListState()

                LazyColumn(Modifier.heightIn(max = Tokens.Size.MenuList).scrollbar(scroll), state = scroll) {
                    items(entries, key = { it.key }) { entry ->
                        when (entry) {
                            is ModelMenuEntry.Heading -> MenuLabel(entry.text)
                            is ModelMenuEntry.Choice -> {
                                val provider = entry.provider
                                val model = entry.model
                                val favourite = model in provider.favourites

                                MenuItem(
                                    model,
                                    onClick = { choose(provider, model) },
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
                        item { MenuLabel("No matching models") }
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
    modifier: Modifier = Modifier,
    error: String? = null,
    trailing: @Composable RowScope.() -> Unit = {},
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
                modifier = Modifier.heightIn(min = 88.dp).padding(top = Tokens.Space.S1),
                alignment = Alignment.TopStart,
                placeholder = placeholder,
            )

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                if (platform != null) {
                    MyButton({ platform.pickFiles(attachments.onAdd) }, icon = MyIcon.Paperclip, description = "Attach files", muted = true)
                }

                Box(Modifier.weight(1f))
                trailing()
                MyButton(onSubmit, icon = MyIcon.Send, description = "Send", muted = true)
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
