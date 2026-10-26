package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.CreateModelProviderRequest
import mycelium.api.ModelProvider
import mycelium.api.UpdateModelProviderRequest
import mycelium.state.Choice
import mycelium.state.Store

private val ProviderKinds = mapOf("OpenAI-compatible" to "openai_compatible", "Anthropic" to "anthropic", "OpenRouter" to "openrouter", "LiteLLM" to "litellm")

private val DataHandling = listOf(Choice("cloud", "Cloud"), Choice("zdr", "Zero data retention"), Choice("private", "Private or on-device only"))

private const val OpenRouterUrl = "https://openrouter.ai/api/v1"

@Composable
fun ModelProvidersSettings(store: Store) {
    val colors = MyTheme.colors
    val scope = rememberCoroutineScope()
    var providers by remember { mutableStateOf<List<ModelProvider>>(emptyList()) }
    var expanded by remember { mutableStateOf<String?>(null) }
    var renaming by remember { mutableStateOf<ModelProvider?>(null) }
    var renamed by remember { mutableStateOf("") }
    var search by remember { mutableStateOf("") }
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
        if (kind == "openrouter" && apiKey.isBlank()) {
            failure = "Enter your OpenRouter API key."

            return
        }

        scope.launch {
            try {
                val request = when (kind) {
                    "openrouter" -> CreateModelProviderRequest("openai_compatible", name.trim().ifEmpty { "OpenRouter" }, OpenRouterUrl, apiKey.trim())
                    "litellm" -> CreateModelProviderRequest("openai_compatible", name.trim().ifEmpty { "LiteLLM" }, baseUrl.trim(), apiKey.trim().ifEmpty { null })
                    else -> CreateModelProviderRequest(kind, name.trim(), baseUrl.trim().ifEmpty { null }.takeIf { kind == "openai_compatible" }, apiKey.trim().ifEmpty { null })
                }
                val added = store.api.createModelProvider(request)
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
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Label("Chat models", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
                MyButton({ adding = true }, text = "Add a provider", icon = MyIcon.Plus, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }

            val columns = listOf(ColumnSpec(max = 280.dp), ColumnSpec(), ColumnSpec(flexible = true)) + ColumnSpec() + ColumnSpec(max = 64.dp)

            ContentTable(columns) {
                ContentRow(header = true) {
                    Label("Provider", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Key", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Models", style = Tokens.Type.MetaStrong, color = colors.textMuted)

                    Label("Data handling", style = Tokens.Type.MetaStrong, color = colors.textMuted)

                    Box(Modifier)
                }

                Hairline()

                for (provider in providers) {
                    val kindName = ProviderKinds.entries.firstOrNull { it.value == provider.kind }?.key ?: provider.kind

                    ContentRow(Modifier.padding(vertical = Tokens.Space.S1_5)) {
                        Column {
                            Label(provider.name, maxLines = 1)
                            Label(provider.baseUrl ?: kindName, style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
                        }

                        Label(provider.keyHint?.let { "Ends in $it" } ?: "No key", color = colors.textMuted, maxLines = 1)
                        MyButton(
                            { expanded = if (expanded == provider.id) null else provider.id; search = "" },
                            text = if (provider.models.isEmpty()) "No models found" else "${provider.models.size} models",
                            description = "${if (expanded == provider.id) "Hide" else "Browse"} models from ${provider.name}",
                            trailingIcon = if (expanded == provider.id) MyIcon.ChevronDown else MyIcon.ChevronRight,
                            muted = true,
                            size = ButtonSize.Sm,
                        )

                        OptionMenu("Data handling", DataHandling, provider.modelClass, { chosen ->
                            store.act {
                                val updated = store.api.updateModelProvider(provider.id, UpdateModelProviderRequest(modelClass = chosen))
                                providers = providers.map { if (it.id == updated.id) updated else it }
                            }
                        }, size = ButtonSize.Sm)

                        RowMenu(provider.name) { close ->
                            MenuItem("Refresh models", onClick = {
                                close()
                                store.act { providers = providers.map { if (it.id == provider.id) store.api.refreshModelProvider(provider.id) else it } }
                            })
                            MenuItem("Rename", onClick = {
                                close()
                                renamed = provider.name
                                renaming = provider
                            })
                            MenuItem("Remove provider", onClick = {
                                close()
                                store.act {
                                    store.api.deleteModelProvider(provider.id)
                                    providers = providers.filter { it.id != provider.id }
                                }
                            })
                        }
                    }

                    Hairline()
                }
            }
        }

        renaming?.let { target ->
            val save = {
                renaming = null
                store.act {
                    val updated = store.api.updateModelProvider(target.id, UpdateModelProviderRequest(name = renamed.trim()))
                    providers = providers.map { if (it.id == updated.id) updated else it }
                }
            }

            RenameDialog("Rename provider", "Provider name", renamed, { renamed = it }, save, { renaming = null })
        }

        val browsing = providers.firstOrNull { it.id == expanded }

        if (browsing != null) {
            Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label("${browsing.name} models", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                TextInput(search, { search = it }, label = "Search ${browsing.name} models")

                val matches = browsing.models.filter { it.contains(search, ignoreCase = true) }

                if (matches.isEmpty()) {
                    Label("No matching models", color = colors.textMuted)
                } else {
                    LazyColumn(Modifier.fillMaxWidth().height(minOf(280, matches.size * 32).dp)) {
                        items(matches) { model ->
                            Label(model, Modifier.padding(vertical = Tokens.Space.S1), Tokens.Type.Code)
                        }
                    }
                }
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
                TextInput(name, { name = it }, label = "Name", placeholder = when (kind) {
                    "anthropic" -> "Anthropic"
                    "openrouter" -> "OpenRouter"
                    "litellm" -> "LiteLLM"
                    else -> "Ollama"
                })

                if (kind == "openai_compatible") {
                    TextInput(baseUrl, { baseUrl = it }, label = "Base URL", placeholder = "http://localhost:11434/v1", mono = true)
                    Label("Ollama serves this at http://<host>:11434/v1.", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                if (kind == "litellm") {
                    TextInput(baseUrl, { baseUrl = it }, label = "Base URL", placeholder = "http://localhost:4000", mono = true)
                    Label("LiteLLM serves this at http://<host>:4000.", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                TextInput(apiKey, { apiKey = it }, label = when (kind) {
                    "openai_compatible" -> "API key, if the endpoint needs one"
                    "litellm" -> "API key, if the proxy needs one"
                    else -> "API key"
                }, mono = true, masked = true, onSubmit = ::add)

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
        }
    }
}
