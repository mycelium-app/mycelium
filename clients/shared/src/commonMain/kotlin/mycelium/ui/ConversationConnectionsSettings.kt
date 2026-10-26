package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
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
import mycelium.api.CreateGroupConnectionRequest
import mycelium.state.Choice
import mycelium.state.Store

@Composable
fun ConversationConnectionsSettings(store: Store) {
    val state by store.state.collectAsState()
    val connections = state.conversationConnections.values.sortedBy { it.createdAt }

    LaunchedEffect(Unit) {
        store.act {
            store.refreshConversationConnections()
            store.refreshGroups()
            store.refreshGroupConnections()
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Agent connections", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("Connected conversations may send messages to each other without approval for each message. Choose Send and connect on a message proposal to allow a connection.", color = MyTheme.colors.textMuted)

        if (connections.isEmpty()) {
            Label("No conversations are connected.", color = MyTheme.colors.textMuted)
        }

        for (connection in connections) {
            ConnectionRow(
                state.conversations[connection.sourceId]?.title ?: connection.sourceTitle,
                state.conversations[connection.targetId]?.title ?: connection.targetTitle,
                "${connection.sourceTitle} and ${connection.targetTitle}",
            ) { store.act { store.revokeConversationConnection(connection.id) } }
        }

        GroupConnectionsBlock(store)
    }
}

@Composable
private fun GroupConnectionsBlock(store: Store) {
    val state by store.state.collectAsState()
    val connections = state.groupConnections.values.sortedBy { it.createdAt }
    val groups = state.groups.values.sortedBy { it.name.lowercase() }
    var connecting by remember { mutableStateOf(false) }
    var first by remember { mutableStateOf("") }
    var second by remember { mutableStateOf("") }
    var failure by remember { mutableStateOf<String?>(null) }
    val attempt = rememberAttempt { failure = it }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Groups", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        Label("Conversations in connected groups may start and message each other without approval.", color = MyTheme.colors.textMuted)

        if (connections.isEmpty()) {
            Label("No groups are connected.", color = MyTheme.colors.textMuted)
        }

        for (connection in connections) {
            ConnectionRow(connection.sourceName, connection.targetName, "${connection.sourceName} and ${connection.targetName}") {
                store.act { store.revokeGroupConnection(connection.id) }
            }
        }

        if (groups.size < 2) {
            Label("Create a second group in Settings, Groups to connect groups.", color = MyTheme.colors.textMuted)
        } else if (!connecting) {
            MyButton({
                failure = null
                first = groups[0].id
                second = groups[1].id
                connecting = true
            }, text = "Connect groups", variant = ButtonVariant.Outline)
        } else {
            val choices = groups.map { Choice(it.id, it.name) }

            failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }
            OptionMenu("First group", choices, first, { first = it })
            OptionMenu("Second group", choices, second, { second = it })
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({
                    if (first == second) {
                        failure = "Choose two different groups."
                    } else {
                        attempt {
                            store.connectGroups(CreateGroupConnectionRequest(first, second))
                            connecting = false
                        }
                    }
                }, text = "Connect", variant = ButtonVariant.Outline)
                MyButton({ connecting = false }, text = "Cancel", muted = true)
            }
        }
    }
}

@Composable
private fun ConnectionRow(first: String, second: String, menuLabel: String, onRevoke: () -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Column(Modifier.weight(1f)) {
            Label(first)
            Label(second, color = MyTheme.colors.textMuted)
        }

        RowMenu(menuLabel) { close ->
            MenuItem("Revoke", {
                close()
                onRevoke()
            })
        }
    }

    Hairline()
}
