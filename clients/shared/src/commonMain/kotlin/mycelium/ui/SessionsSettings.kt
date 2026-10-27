package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.NodeSummary
import mycelium.api.SessionSummary
import mycelium.state.Store
import mycelium.state.apiScopeLabel

@Composable
fun SessionsSettings(store: Store, onSignOut: () -> Unit) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    val stamp = timestamper()
    var sessions by remember { mutableStateOf<List<SessionSummary>>(emptyList()) }
    var loaded by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    val loadFailures by store.loadFailures.collectAsState()
    val shown = sessions.filter { it.label.contains(query.trim(), ignoreCase = true) || it.hostname.contains(query.trim(), ignoreCase = true) }
    val load = { store.load("sessions") { sessions = store.api.sessions(); loaded = true } }
    var endingCurrent by remember { mutableStateOf<SessionSummary?>(null) }
    var revoking by remember { mutableStateOf<NodeSummary?>(null) }

    LaunchedEffect(Unit) {
        load()
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Clients", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            ListSearch(query, { query = it }, "sessions")

            ListFrame("sessions", loaded, "sessions" in loadFailures, shown.size, load, if (query.isBlank()) "No sessions." else "No sessions match.") {
                Table(
                    listOf(
                        Col("Client", 120.dp, weight = 1f, overflow = Overflow.Wrap),
                        Col("Address", 100.dp, max = 240.dp, priority = 1),
                        Col("Last used", 88.dp),
                        ActionColumn,
                    ),
                    shown.size,
                    { row, column ->
                        val session = shown[row]

                        val where = when {
                            session.kind == "agent" -> session.conversationTitle?.let { "${session.label} for $it" } ?: session.label
                            session.hostname.isBlank() -> session.label
                            else -> "${session.label} on ${session.hostname}"
                        }

                        when (column) {
                            0 -> TableCell.Slot(where, Tokens.Size.IconSm + Tokens.Space.S1_5) {
                                Column {
                                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                        Icon(
                                            when (session.kind) {
                                                "browser" -> MyIcon.Globe
                                                "agent" -> MyIcon.Key
                                                else -> MyIcon.Laptop
                                            },
                                            colors.textMuted,
                                            Tokens.Size.IconSm,
                                        )
                                        Label(where, Modifier.weight(1f, fill = false), maxLines = 1)

                                        if (session.current) {
                                            Label("This session", style = Tokens.Type.Meta, color = colors.textMuted)
                                        }
                                    }

                                    session.expiresAt?.let { expiresAt ->
                                        Label("${session.scopes.joinToString(", ") { apiScopeLabel(it) }}. Expires ${timestamp(expiresAt)}", style = Tokens.Type.Meta, color = colors.textMuted)
                                    }
                                }
                            }

                            1 -> TableCell.Text(session.address, Tokens.Type.Code, colors.textMuted)
                            2 -> TableCell.Text(if (session.current) "now" else stamp(session.lastUsedAt, true), color = colors.textMuted)
                            else -> TableCell.Slot {
                                RowMenu(where) { close ->
                                    MenuItem(if (session.kind == "agent") "Revoke" else "End session", {
                                        close()
                                        if (session.current) {
                                            endingCurrent = session
                                        } else {
                                            store.act {
                                                store.api.endSession(session.id)
                                                sessions = sessions.filter { it.id != session.id }
                                            }
                                        }
                                    })
                                }
                            }
                        }
                    },
                )
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Nodes", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            val nodes = state.nodes.values.sortedBy { it.name }

            Table(
                listOf(
                    Col("Node", 120.dp, weight = 1f),
                    Col("Address", 100.dp, max = 240.dp, priority = 1),
                    Col("Last seen", 88.dp),
                    ActionColumn,
                ),
                nodes.size,
                { row, column ->
                    val node = nodes[row]

                    when (column) {
                        0 -> TableCell.Slot(node.name, Tokens.Size.IconSm + Tokens.Space.S1_5) {
                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                                Icon(nodeIcon(node), colors.textMuted, Tokens.Size.IconSm)
                                Label(node.name, maxLines = 1)
                            }
                        }

                        1 -> TableCell.Text(node.address, Tokens.Type.Code, colors.textMuted)
                        2 -> TableCell.Text(if (node.online) "online" else node.lastSeenAt?.let { stamp(it, true) } ?: "never", color = colors.textMuted)
                        else -> TableCell.Slot {
                            RowMenu(node.name) { close ->
                                MenuItem("Revoke", {
                                    close()
                                    revoking = node
                                })
                            }
                        }
                    }
                },
            )
        }
    }

    endingCurrent?.let { session ->
        ConfirmDialog("Sign out of this session?", "This is the session you are using. Ending it signs you out here.", "Sign out", { endingCurrent = null }) {
            endingCurrent = null
            store.act {
                store.api.endSession(session.id, confirmCurrent = true)
                onSignOut()
            }
        }
    }

    revoking?.let { node ->
        ConfirmDialog("Revoke ${node.name}?", "It disconnects now and must join again to come back.", "Revoke ${node.name}", { revoking = null }) {
            revoking = null
            store.act { store.api.revokeNode(node.id) }
        }
    }
}
