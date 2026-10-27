package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.CreateDecisionProviderRequest
import mycelium.api.DecisionProvider
import mycelium.api.PatchDecisionProviderRequest
import mycelium.api.TestDecisionProviderRequest
import mycelium.state.Choice
import mycelium.state.Store

private val DecisionUses = listOf(
    Choice("decide", "The decide tool, for agents"),
)

private val UseSummaries = mapOf("decide" to "decide")

@Composable
fun DecisionsSettings(store: Store) {
    val colors = MyTheme.colors
    var providers by remember { mutableStateOf<List<DecisionProvider>>(emptyList()) }
    var editing by remember { mutableStateOf<DecisionDraft?>(null) }
    var removing by remember { mutableStateOf<DecisionProvider?>(null) }

    LaunchedEffect(Unit) {
        store.act { providers = store.api.decisionProviders() }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Label("Decisions", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
            MyButton({ editing = DecisionDraft() }, text = "Add a decisions provider", icon = MyIcon.Plus, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        Label("An OpenJev or Jev-compatible server that answers yes or no, choice and score questions with a probability. Agents ask it through the decide tool.", style = Tokens.Type.Meta, color = colors.textMuted)

        Table(
            listOf(
                Col("Provider", 110.dp, max = 280.dp, weight = 1f),
                Col("Key", 88.dp, priority = 1),
                Col("Model", 120.dp, rigid = true, priority = 1),
                Col("Used for", 120.dp, weight = 1f),
                ActionColumn,
            ),
            providers.size,
            { row, column ->
                val provider = providers[row]

                when (column) {
                    0 -> TableCell.Slot(provider.baseUrl) {
                        Column {
                            Label(provider.name, maxLines = 1)
                            Label(provider.baseUrl, style = Tokens.Type.Meta, color = colors.textFaint, maxLines = 1)
                        }
                    }

                    1 -> TableCell.Text(provider.keyHint?.let { "Ends in $it" } ?: "No key", color = colors.textMuted)
                    2 -> TableCell.Text(provider.model, Tokens.Type.Code)
                    3 -> TableCell.Text(provider.uses.joinToString(", ") { UseSummaries[it] ?: it }.ifEmpty { "Nothing yet" }, color = colors.textMuted)
                    else -> TableCell.Slot {
                        RowMenu(provider.name) { close ->
                            MenuItem("Edit", onClick = {
                                close()
                                editing = DecisionDraft(provider.id, provider.name, provider.baseUrl, provider.model, provider.uses.toSet())
                            })
                            MenuItem("Remove provider", onClick = {
                                close()
                                removing = provider
                            })
                        }
                    }
                }
            },
        )
    }

    editing?.let { draft ->
        DecisionProviderDialog(store, draft, { editing = null }) { saved ->
            providers = providers.filter { it.id != saved.id }.map { it.copy(uses = it.uses - saved.uses.toSet()) } + saved
            editing = null
        }
    }

    removing?.let { target ->
        ConfirmDialog("Remove ${target.name}?", "Agents lose the decide tool until another provider takes over.", "Remove", { removing = null }) {
            removing = null
            store.act {
                store.api.deleteDecisionProvider(target.id)
                providers = providers.filter { it.id != target.id }
            }
        }
    }
}

private data class DecisionDraft(
    val id: String? = null,
    val name: String = "",
    val address: String = "",
    val model: String = "",
    val uses: Set<String> = setOf("decide"),
)

@Composable
private fun DecisionProviderDialog(store: Store, draft: DecisionDraft, onDismiss: () -> Unit, onSaved: (DecisionProvider) -> Unit) {
    val colors = MyTheme.colors
    var name by remember { mutableStateOf(draft.name) }
    var address by remember { mutableStateOf(draft.address) }
    var key by remember { mutableStateOf("") }
    var models by remember { mutableStateOf(if (draft.model.isEmpty()) emptyList() else listOf(draft.model)) }
    var model by remember { mutableStateOf(draft.model) }
    var uses by remember { mutableStateOf(draft.uses) }
    var tested by remember { mutableStateOf<String?>(null) }
    val runner = rememberAction()

    fun test() {
        tested = null

        runner {
            val found = store.api.testDecisionProvider(TestDecisionProviderRequest(address.trim(), key.trim().ifEmpty { null })).models
            models = found
            model = model.takeIf { it in found } ?: found.firstOrNull().orEmpty()
            tested = "Connected. The server offers ${found.size} ${if (found.size == 1) "model" else "models"}."
            runner.failure = null
        }
    }

    fun save() {
        runner {
            val saved = if (draft.id == null) {
                store.api.createDecisionProvider(CreateDecisionProviderRequest(name.trim(), address.trim(), key.trim().ifEmpty { null }, model, DecisionUses.map { it.id }.filter { it in uses }))
            } else {
                store.api.updateDecisionProvider(draft.id, PatchDecisionProviderRequest(name.trim(), address.trim(), key.trim().ifEmpty { null }, model, DecisionUses.map { it.id }.filter { it in uses }))
            }

            onSaved(saved)
        }
    }

    Dialog(onDismiss) {
        Label("Decisions provider", Modifier.semantics { heading() }, Tokens.Type.Title)
        TextInput(name, { name = it }, label = "Name", placeholder = "OpenJev")
        TextInput(address, { address = it; tested = null }, label = "Address", placeholder = "http://localhost:8080", mono = true)
        TextInput(key, { key = it; tested = null }, label = if (draft.id == null) "Key, if the server needs one" else "Key, leave blank to keep the saved one", mono = true, masked = true)
        Label("Sent as a bearer token", style = Tokens.Type.Meta, color = colors.textMuted)

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2), itemVerticalAlignment = Alignment.CenterVertically) {
            MyButton(::test, text = "Test", icon = MyIcon.Check, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            tested?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted) }
        }

        if (models.isNotEmpty()) {
            Label("Model", style = Tokens.Type.Meta, color = colors.textMuted)
            OptionMenu("Model", models.map { Choice(it, it) }, model, { model = it })
        }

        Label("Use it for", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        for (use in DecisionUses) {
            val on = use.id in uses

            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(on, use.name, { uses = if (on) uses - use.id else uses + use.id }, checkedState = "On", uncheckedState = "Off")
                Label(use.name)
            }
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}
