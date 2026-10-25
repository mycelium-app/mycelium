package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
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
import kotlinx.coroutines.CancellationException
import mycelium.api.ApiException
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.Memory
import mycelium.state.Store

@Composable
fun MemoriesSettings(store: Store) {
    val colors = MyTheme.colors
    val touch = LocalTouch.current
    val live by store.state.collectAsState()
    var memories by remember { mutableStateOf<List<Memory>>(emptyList()) }
    var loaded by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    var editing by remember { mutableStateOf<String?>(null) }
    var draft by remember { mutableStateOf("") }
    var deleting by remember { mutableStateOf<Memory?>(null) }
    var failure by remember { mutableStateOf<String?>(null) }
    var retry by remember { mutableStateOf(0) }

    val searching = query.isNotBlank()
    val shown = if (searching) {
        memories.filter { it.id !in live.removedMemories }
    } else {
        val held = memories.map { it.id }.toSet()
        val arrived = live.memories.values.filter { it.id !in held }.sortedByDescending { it.createdAt }

        (arrived + memories.map { live.memories[it.id] ?: it }).filter { it.id !in live.removedMemories }
    }

    LaunchedEffect(query, if (searching) live.memoryRevision else 0, retry) {
        failure = null

        try {
            memories = store.api.memories(query)
            loaded = true
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (problem: ApiException) {
            failure = problem.message
        } catch (_: Exception) {
            failure = "Could not load memories. Check your connection and try again."
        }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
            if (!touch) {
                Label("Memories", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
            }

            TextInput(query, { query = it }, label = "Search memories", placeholder = "Search memories", showLabel = false,
                modifier = if (touch) Modifier.fillMaxWidth() else Modifier.widthIn(max = Tokens.Size.FormColumn))
        }

        failure?.let { message ->
            ErrorBanner(message, onDismiss = { failure = null })
            MyButton({ retry++ }, text = "Retry", variant = ButtonVariant.Outline)
        }

        Column(Modifier.fillMaxWidth()) {
            if (loaded && shown.isEmpty() && failure == null) {
                Label(
                    if (query.isBlank()) "Nothing remembered yet. Agents add memories with the remember tool." else "No memories match.",
                    color = colors.textMuted,
                )
            }

            for (memory in shown) {
                Column(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                    if (editing == memory.id) {
                        fun save() {
                            val text = draft.trim()

                            store.act {
                                val saved = store.api.updateMemory(memory.id, text)
                                memories = memories.map { if (it.id == saved.id) saved else it }
                                editing = null
                            }
                        }

                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                            TextInput(draft, { draft = it }, label = "Memory text", showLabel = false, modifier = Modifier.weight(1f), onSubmit = ::save)
                            MyButton(::save, text = "Save", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                            MyButton({ editing = null }, text = "Cancel", size = ButtonSize.Sm, muted = true)
                        }
                    } else {
                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
                            Label(memory.text, Modifier.weight(1f).padding(top = Tokens.Space.S1))
                            RowMenu(memory.text) { close ->
                                MenuItem("Edit", {
                                    close()
                                    editing = memory.id
                                    draft = memory.text
                                    deleting = null
                                })
                                MenuItem("Delete", {
                                    close()
                                    deleting = memory
                                    editing = null
                                })
                            }
                        }
                    }

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        for (scope in memory.scopes) {
                            Tag(scope)
                        }

                        Label("${memory.createdBy}  ·  ${timestamp(memory.createdAt)}", Modifier.weight(1f, fill = false), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                    }
                }

                Hairline()
            }
        }
    }

    deleting?.let { memory ->
        Dialog(onDismiss = { deleting = null }) {
            Label("Delete this memory? Agents will no longer recall it.", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({
                    store.act {
                        store.api.deleteMemory(memory.id)
                        memories = memories.filter { it.id != memory.id }
                        deleting = null
                    }
                }, text = "Delete memory", variant = ButtonVariant.Danger)
                MyButton({ deleting = null }, text = "Cancel", muted = true)
            }
        }
    }
}
