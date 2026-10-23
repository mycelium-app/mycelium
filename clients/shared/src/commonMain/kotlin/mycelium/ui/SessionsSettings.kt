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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.NodeSummary
import mycelium.api.SessionSummary
import mycelium.state.Store
import mycelium.state.ago

@Composable
fun SessionsSettings(store: Store, onSignOut: () -> Unit) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var sessions by remember { mutableStateOf<List<SessionSummary>>(emptyList()) }
    var endingCurrent by remember { mutableStateOf<SessionSummary?>(null) }
    var revoking by remember { mutableStateOf<NodeSummary?>(null) }

    LaunchedEffect(Unit) {
        store.act { sessions = store.api.sessions() }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Clients", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Label("Client", Modifier.weight(3f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Address", Modifier.weight(1.5f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Last used", Modifier.weight(1f), Tokens.Type.MetaStrong, colors.textMuted)
                Box(Modifier.weight(1.5f))
            }

            Hairline()

            for (session in sessions) {
                val where = if (session.hostname.isBlank()) session.label else "${session.label} on ${session.hostname}"

                Row(
                    Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Row(Modifier.weight(3f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                        Icon(if (session.kind == "browser") MyIcon.Globe else MyIcon.Laptop, colors.textMuted, Tokens.Size.IconSm)
                        Label(where, Modifier.weight(1f, fill = false), maxLines = 1)

                        if (session.current) {
                            Label("This session", style = Tokens.Type.Meta, color = colors.textMuted)
                        }
                    }

                    Label(session.address, Modifier.weight(1.5f), Tokens.Type.Code, colors.textMuted, maxLines = 1)
                    Label(if (session.current) "now" else ago(session.lastUsedAt), Modifier.weight(1f), color = colors.textMuted)

                    Row(Modifier.weight(1.5f), horizontalArrangement = Arrangement.End) {
                        MyButton(
                            {
                                if (session.current) {
                                    endingCurrent = session
                                } else {
                                    store.act {
                                        store.api.endSession(session.id)
                                        sessions = sessions.filter { it.id != session.id }
                                    }
                                }
                            },
                            text = "End session",
                            size = ButtonSize.Sm,
                            variant = ButtonVariant.Outline,
                            description = "End session $where",
                        )
                    }
                }

                Hairline()
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Nodes", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Label("Node", Modifier.weight(3f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Address", Modifier.weight(1.5f), Tokens.Type.MetaStrong, colors.textMuted)
                Label("Last seen", Modifier.weight(1f), Tokens.Type.MetaStrong, colors.textMuted)
                Box(Modifier.weight(1.5f))
            }

            Hairline()

            for (node in state.nodes.values.sortedBy { it.name }) {
                Row(
                    Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2),
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Row(Modifier.weight(3f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                        Icon(nodeIcon(node), colors.textMuted, Tokens.Size.IconSm)
                        Label(node.name, maxLines = 1)
                    }

                    Label(node.address, Modifier.weight(1.5f), Tokens.Type.Code, colors.textMuted, maxLines = 1)
                    Label(if (node.online) "online" else node.lastSeenAt?.let(::ago) ?: "never", Modifier.weight(1f), color = colors.textMuted)

                    Row(Modifier.weight(1.5f), horizontalArrangement = Arrangement.End) {
                        MyButton({ revoking = node }, text = "Revoke", size = ButtonSize.Sm, variant = ButtonVariant.Outline, description = "Revoke ${node.name}")
                    }
                }

                Hairline()
            }
        }
    }

    endingCurrent?.let { session ->
        Dialog(onDismiss = { endingCurrent = null }) {
            Label("Sign out of this session?", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Label("This is the session you are using. Ending it signs you out here.", color = colors.textMuted)

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton(
                    {
                        endingCurrent = null
                        store.act {
                            store.api.endSession(session.id, confirmCurrent = true)
                            onSignOut()
                        }
                    },
                    text = "Sign out",
                    variant = ButtonVariant.Danger,
                )
                MyButton({ endingCurrent = null }, text = "Cancel", muted = true)
            }
        }
    }

    revoking?.let { node ->
        Dialog(onDismiss = { revoking = null }) {
            Label("Revoke ${node.name}?", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Label("It disconnects now and must join again to come back.", color = colors.textMuted)

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton(
                    {
                        revoking = null
                        store.act { store.api.revokeNode(node.id) }
                    },
                    text = "Revoke ${node.name}",
                    variant = ButtonVariant.Danger,
                )
                MyButton({ revoking = null }, text = "Cancel", muted = true)
            }
        }
    }
}
