package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.state.Store

@Composable
fun ConversationConnectionsSettings(store: Store) {
    val state by store.state.collectAsState()
    val connections = state.conversationConnections.values.sortedBy { it.createdAt }

    LaunchedEffect(Unit) {
        store.act { store.refreshConversationConnections() }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Agent connections", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("Connected conversations may send messages to each other without approval for each message. Choose Send and connect on a message proposal to allow a connection.", color = MyTheme.colors.textMuted)

        if (connections.isEmpty()) {
            Label("No conversations are connected.", color = MyTheme.colors.textMuted)
        }

        for (connection in connections) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Column(Modifier.weight(1f)) {
                    Label(state.conversations[connection.sourceId]?.title ?: connection.sourceTitle)
                    Label(state.conversations[connection.targetId]?.title ?: connection.targetTitle, color = MyTheme.colors.textMuted)
                }

                RowMenu("${connection.sourceTitle} and ${connection.targetTitle}") { close ->
                    MenuItem("Revoke", {
                        close()
                        store.act { store.revokeConversationConnection(connection.id) }
                    })
                }
            }

            Hairline()
        }
    }
}
