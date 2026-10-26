package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.TextFieldValue
import mycelium.api.Conversation
import mycelium.api.CreateChatRequest
import mycelium.api.ModelProvider
import mycelium.state.Store

@Composable
fun NewChatScreen(store: Store, onCreated: (Conversation) -> Unit, onOpenSettings: () -> Unit = {}) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var providers by remember { mutableStateOf<List<ModelProvider>?>(null) }
    var providerId by remember { mutableStateOf<String?>(null) }
    var model by remember { mutableStateOf<String?>(null) }
    var message by remember { mutableStateOf(TextFieldValue("")) }
    val attached = rememberAttachmentDrafts()
    var messageError by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        store.act { providers = store.api.modelProviders() }
    }

    val loaded = providers
    val provider = loaded?.firstOrNull { it.id == providerId } ?: loaded?.firstOrNull { it.models.isNotEmpty() }
    val chosenModel = provider?.models?.firstOrNull { it == model } ?: provider?.defaultModel?.takeIf { it in provider.models } ?: provider?.models?.firstOrNull()

    fun create() {
        if (provider == null || chosenModel == null) {
            return
        }

        val text = message.text.trim()

        if (text.isEmpty() && attached.drafts.isEmpty()) {
            messageError = "Enter a message to start the chat."
            return
        }

        messageError = null
        val files = attached.drafts.map { it.file }

        store.act {
            if (files.isEmpty()) {
                onCreated(store.api.createChat(CreateChatRequest(provider.id, chosenModel, text)))
            } else {
                val created = store.api.createChat(CreateChatRequest(provider.id, chosenModel))
                store.send(created.id, text, files)
                onCreated(created)
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("New chat")
        }

        if (loaded == null) {
            return@Column
        }

        if (provider == null) {
            Column(Modifier.padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Label("No chat model is set up. Add a provider in Settings, under Chat models, then start a chat.", color = colors.textMuted)

                Row {
                    MyButton(onOpenSettings, text = "Open Settings", icon = MyIcon.Settings, variant = ButtonVariant.Outline)
                }
            }

            return@Column
        }

        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
            Column(
                Modifier.widthIn(max = readingMeasure()).fillMaxWidth().padding(Tokens.Space.S4),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6),
            ) {
                Label("What's on your mind?", style = Tokens.Type.Display)

                ComposerCard(
                    value = message,
                    onValueChange = { message = it; messageError = null },
                    placeholder = "Ask anything",
                    onSubmit = ::create,
                    attachments = attached.controls(),
                    error = messageError,
                    trailing = {
                        ModelPicker(
                            ModelPickerControls(
                                loaded,
                                provider.id,
                                chosenModel,
                                recent = recentModels(state.conversations.values, loaded),
                                onFavourite = { picked, favouriteModel, on -> store.setFavouriteModel({ change -> providers = providers?.let(change) }, picked, favouriteModel, on) },
                            ) { picked, chosenPick -> providerId = picked.id; model = chosenPick },
                        )
                    },
                )
            }
        }
    }
}
