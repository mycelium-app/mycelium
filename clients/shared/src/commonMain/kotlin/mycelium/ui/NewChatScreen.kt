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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.api.Conversation
import mycelium.api.CreateChatRequest
import mycelium.api.ModelProvider
import mycelium.state.Choice
import mycelium.state.Store

@Composable
fun NewChatScreen(store: Store, onCreated: (Conversation) -> Unit, onOpenSettings: () -> Unit = {}) {
    val colors = MyTheme.colors
    var providers by remember { mutableStateOf<List<ModelProvider>?>(null) }
    var providerId by remember { mutableStateOf<String?>(null) }
    var model by remember { mutableStateOf<String?>(null) }
    var message by remember { mutableStateOf("") }
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

        if (message.isBlank()) {
            messageError = "Enter a message to start the chat."
            return
        }

        messageError = null
        store.act { onCreated(store.api.createChat(CreateChatRequest(provider.id, chosenModel, message.trim()))) }
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

        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
            Column(
                Modifier.widthIn(max = Tokens.Size.Page).fillMaxWidth().padding(Tokens.Space.S4),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label("Model", style = Tokens.Type.MetaStrong, color = colors.textMuted)

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        OptionMenu("Provider", loaded.map { Choice(it.id, it.name) }, provider.id, { providerId = it; model = null })
                        OptionMenu("Model", provider.models.map { Choice(it, it) }, chosenModel, { model = it })
                    }
                }

                TextInput(message, { message = it }, label = "Message", placeholder = "Ask anything", error = messageError, onSubmit = ::create)

                Row {
                    MyButton(::create, text = "Start chat", variant = ButtonVariant.Primary, size = ButtonSize.Lg)
                }
            }
        }
    }
}
