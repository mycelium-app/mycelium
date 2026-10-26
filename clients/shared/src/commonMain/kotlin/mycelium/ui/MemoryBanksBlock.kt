package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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
import mycelium.api.MemoryBank
import mycelium.api.SaveMemoryBankRequest
import mycelium.state.Choice
import mycelium.state.Store

val MemoryModelClasses = listOf(
    Choice("private", "Private or on-device only"),
    Choice("zdr", "Zero data retention or stricter"),
    Choice("cloud", "Any model"),
)

private class BankDraft(val editing: MemoryBank?) {
    var name by mutableStateOf(editing?.name.orEmpty())
    var modelClass by mutableStateOf(editing?.modelClass ?: "cloud")
    var groupIds by mutableStateOf(editing?.groupIds.orEmpty().toSet())
}

@Composable
fun MemoryBanksBlock(store: Store) {
    val colors = MyTheme.colors
    val live by store.state.collectAsState()
    val banks = live.memoryBanks.values.sortedBy { it.name.lowercase() }
    val groups = live.groups.values.sortedBy { it.name.lowercase() }
    var drafting by remember { mutableStateOf<BankDraft?>(null) }
    var deleting by remember { mutableStateOf<MemoryBank?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    val attempt = rememberAttempt { failure = it }

    LaunchedEffect(Unit) {
        store.act {
            store.refreshGroups()
            store.refreshMemoryBanks()
        }
    }

    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Label("Banks", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
            MyButton({ failure = null; drafting = BankDraft(null) }, text = "New bank", icon = MyIcon.Plus, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }

        if (banks.isNotEmpty()) {
            ContentTable(listOf(ColumnSpec(max = 240.dp), ColumnSpec(flexible = true), ColumnSpec(max = 280.dp), ColumnSpec(max = 64.dp))) {
                ContentRow(header = true) {
                    Label("Name", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Models", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Label("Groups", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    Box(Modifier)
                }

                Hairline()

                for (bank in banks) {
                    ContentRow(Modifier.padding(vertical = Tokens.Space.S1_5)) {
                        Label(bank.name, maxLines = 1)
                        Label(MemoryModelClasses.first { it.id == bank.modelClass }.name, color = colors.textMuted, maxLines = 1)
                        Label(if (bank.groupIds.isEmpty()) "Every group" else bank.groupIds.mapNotNull { live.groups[it]?.name }.joinToString(", "), color = colors.textMuted, maxLines = 1)

                        RowMenu("${bank.name} bank") { close ->
                            MenuItem("Edit", onClick = { close(); failure = null; drafting = BankDraft(bank) })
                            MenuItem("Delete", onClick = { close(); failure = null; deleting = bank })
                        }
                    }

                    Hairline()
                }
            }
        }
    }

    drafting?.let { draft ->
        Dialog(onDismiss = { drafting = null }) {
            fun save() {
                val request = SaveMemoryBankRequest(draft.name.trim(), draft.modelClass, draft.groupIds.toList())

                attempt {
                    val saved = draft.editing?.let { store.api.updateMemoryBank(it.id, request) } ?: store.api.createMemoryBank(request)
                    store.receive(mycelium.api.StreamMessage.MemoryBankChanged(saved))
                    drafting = null
                }
            }

            Label(if (draft.editing == null) "New bank" else "Edit bank", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }
            TextInput(draft.name, { draft.name = it }, label = "Name", onSubmit = ::save)
            OptionMenu("Models", MemoryModelClasses, draft.modelClass, { draft.modelClass = it })

            if (groups.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label("Groups that can use this bank. Choose none for every group.", color = colors.textMuted)

                    for (group in groups) {
                        val chosen = group.id in draft.groupIds

                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                            Checkbox(chosen, group.name, { draft.groupIds = if (chosen) draft.groupIds - group.id else draft.groupIds + group.id }, checkedState = "Allowed", uncheckedState = "Not allowed")
                            Label(group.name)
                        }
                    }
                }
            }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton(::save, text = if (draft.editing == null) "Create bank" else "Save bank", variant = ButtonVariant.Outline)
                MyButton({ drafting = null }, text = "Cancel", muted = true)
            }
        }
    }

    deleting?.let { bank ->
        Dialog(onDismiss = { deleting = null }) {
            Label("Delete the ${bank.name} bank?", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({
                    attempt {
                        store.api.deleteMemoryBank(bank.id)
                        store.receive(mycelium.api.StreamMessage.MemoryBankRemoved(bank.id))
                        deleting = null
                    }
                }, text = "Delete bank", variant = ButtonVariant.Danger)
                MyButton({ deleting = null }, text = "Cancel", muted = true)
            }
        }
    }
}
