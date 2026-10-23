package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.CreateModelProviderRequest
import mycelium.api.ModelProvider
import mycelium.state.Store

private val ProviderKinds = mapOf("OpenAI-compatible" to "openai_compatible", "Anthropic" to "anthropic")

@Composable
fun ModelProvidersSettings(store: Store) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    var providers by remember { mutableStateOf<List<ModelProvider>>(emptyList()) }
    var adding by remember { mutableStateOf(false) }
    var kind by remember { mutableStateOf("openai_compatible") }
    var name by remember { mutableStateOf("") }
    var baseUrl by remember { mutableStateOf("") }
    var apiKey by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        store.act { providers = store.api.modelProviders() }
    }

    fun add() {
        scope.launch {
            try {
                val added = store.api.createModelProvider(CreateModelProviderRequest(kind, name.trim(), baseUrl.trim().ifEmpty { null }.takeIf { kind == "openai_compatible" }, apiKey.trim().ifEmpty { null }))
                providers = providers.filter { it.id != added.id } + added
                adding = false
                name = ""
                baseUrl = ""
                apiKey = ""
                failure = null
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: ApiException) {
                failure = problem.message
            } catch (_: Exception) {
                failure = "Could not reach the control plane. Check your connection and try again."
            }
        }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Chat models", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Label("Provider", Modifier.weight(2f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Key", Modifier.weight(1f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Models", Modifier.weight(3f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("", Modifier.weight(2f))
            }

            Hairline()

            for (provider in providers) {
                val kindName = ProviderKinds.entries.first { it.value == provider.kind }.key

                Row(
                    Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(2f)) {
                        Label(provider.name, maxLines = 1)
                        Label(provider.baseUrl ?: kindName, style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
                    }

                    Label(provider.keyHint?.let { "Ends in $it" } ?: "No key", Modifier.weight(1f), color = colors.textMuted, maxLines = 1)
                    Label(provider.models.joinToString(", ").ifEmpty { "No models found" }, Modifier.weight(3f), color = colors.textMuted)

                    Row(Modifier.weight(2f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1, Alignment.End)) {
                        MyButton(
                            { store.act { providers = providers.map { if (it.id == provider.id) store.api.refreshModelProvider(provider.id) else it } } },
                            text = "Refresh",
                            size = ButtonSize.Sm,
                            variant = ButtonVariant.Outline,
                            description = "Refresh the models of ${provider.name}",
                        )
                        MyButton(
                            {
                                store.act {
                                    store.api.deleteModelProvider(provider.id)
                                    providers = providers.filter { it.id != provider.id }
                                }
                            },
                            text = "Remove",
                            size = ButtonSize.Sm,
                            variant = ButtonVariant.Outline,
                            description = "Remove ${provider.name}",
                        )
                    }
                }

                Hairline()
            }
        }

        if (adding) {
            Column(Modifier.widthIn(max = Tokens.Size.FormColumn), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Segmented(
                    ProviderKinds.keys.toList(),
                    ProviderKinds.entries.first { it.value == kind }.key,
                    { kind = ProviderKinds.getValue(it) },
                    groupLabel = "Provider type",
                )
                TextInput(name, { name = it }, label = "Name", placeholder = if (kind == "anthropic") "Anthropic" else "Ollama")

                if (kind == "openai_compatible") {
                    TextInput(baseUrl, { baseUrl = it }, label = "Base URL", placeholder = "http://localhost:11434/v1", mono = true)
                    Label("Ollama serves this at http://<host>:11434/v1.", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                TextInput(apiKey, { apiKey = it }, label = if (kind == "anthropic") "API key" else "API key, if the endpoint needs one", mono = true, masked = true, onSubmit = ::add)

                failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    MyButton(::add, text = "Add provider", variant = ButtonVariant.Outline)
                    MyButton(
                        {
                            adding = false
                            name = ""
                            baseUrl = ""
                            apiKey = ""
                            failure = null
                        },
                        text = "Cancel",
                        muted = true,
                    )
                }
            }
        } else {
            Row {
                MyButton({ adding = true }, text = "Add a provider", icon = MyIcon.Plus, variant = ButtonVariant.Outline)
            }
        }
    }
}
