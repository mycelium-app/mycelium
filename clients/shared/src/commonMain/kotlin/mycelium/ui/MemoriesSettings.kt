package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.api.Memory
import mycelium.state.Store
import mycelium.state.relativeTime

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
    var deleting by remember { mutableStateOf<String?>(null) }

    val searching = query.isNotBlank()
    val shown = if (searching) {
        memories.filter { it.id !in live.removedMemories }
    } else {
        val held = memories.map { it.id }.toSet()
        val arrived = live.memories.values.filter { it.id !in held }.sortedByDescending { it.createdAt }

        (arrived + memories.map { live.memories[it.id] ?: it }).filter { it.id !in live.removedMemories }
    }

    LaunchedEffect(query, if (searching) live.memoryRevision else 0) {
        val asked = query

        store.act {
            val found = store.api.memories(asked)

            if (asked == query) {
                memories = found
                loaded = true
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Memories")

            if (!touch) {
                Box(Modifier.weight(1f))
            }

            TextInput(
                query,
                { query = it },
                label = "Search memories",
                placeholder = "Search memories",
                showLabel = false,
                modifier = if (touch) Modifier.weight(1f) else Modifier.width(Tokens.Size.FormColumn),
            )
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar()) {
            if (loaded && shown.isEmpty()) {
                Label(
                    if (query.isBlank()) "Nothing remembered yet. Agents add memories with the remember tool." else "No memories match.",
                    Modifier.padding(Tokens.Space.S4),
                    color = colors.textMuted,
                )
            }

            for (memory in shown) {
                Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
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
                            MyButton({ editing = memory.id; draft = memory.text; deleting = null }, text = "Edit", size = ButtonSize.Sm, muted = true, description = "Edit memory: ${memory.text}")
                            MyButton({ deleting = memory.id; editing = null }, text = "Delete", size = ButtonSize.Sm, muted = true, description = "Delete memory: ${memory.text}")
                        }
                    }

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        for (scope in memory.scopes) {
                            Tag(scope)
                        }

                        Label("${memory.createdBy}  ·  ${relativeTime(memory.createdAt)}", Modifier.weight(1f, fill = false), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                    }

                    if (deleting == memory.id) {
                        Row(
                            Modifier.fillMaxWidth().heightIn(min = rowHeight()),
                            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Label("Delete this memory? Agents will no longer recall it.", Modifier.weight(1f), color = colors.statusFailed)
                            MyButton(
                                {
                                    deleting = null
                                    store.act {
                                        store.api.deleteMemory(memory.id)
                                        memories = memories.filter { it.id != memory.id }
                                    }
                                },
                                text = "Delete memory",
                                variant = ButtonVariant.Danger,
                                size = ButtonSize.Sm,
                            )
                            MyButton({ deleting = null }, text = "Cancel", size = ButtonSize.Sm, muted = true)
                        }
                    }
                }

                Hairline()
            }
        }
    }
}
