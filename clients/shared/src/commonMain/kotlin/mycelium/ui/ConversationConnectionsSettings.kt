package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
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
        Label("Connected conversations may send messages to each other without approval for each message, until a week after the last one. Choose Send and connect on a message proposal to allow a connection.", color = MyTheme.colors.textMuted)

        if (connections.isEmpty()) {
            Label("No conversations are connected.", color = MyTheme.colors.textMuted)
        }

        for (connection in connections) {
            ConnectionRow(
                state.conversations[connection.sourceId]?.title ?: connection.sourceTitle,
                state.conversations[connection.targetId]?.title ?: connection.targetTitle,
                "${connection.sourceTitle} and ${connection.targetTitle}",
                note = if (connection.held) "Kept while the managed conversation is busy" else connection.expiresAt?.let(::expiryDate)?.let { "Expires $it" },
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
    var direction by remember { mutableStateOf(DirectionBoth) }
    val runner = rememberAction()

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Groups", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        Label("Conversations in connected groups may start and message each other without approval.", color = MyTheme.colors.textMuted)

        if (connections.isEmpty()) {
            Label("No groups are connected.", color = MyTheme.colors.textMuted)
        }

        for (connection in connections) {
            val current = if (connection.bothWays) DirectionBoth else DirectionForward
            val choices = directionChoices(connection.sourceName, connection.targetName)

            ConnectionRow(
                connection.sourceName,
                connection.targetName,
                "${connection.sourceName} and ${connection.targetName}",
                note = choices.first { it.id == current }.name,
                menuExtra = { close ->
                    MenuLabel("Direction")

                    for (choice in choices) {
                        MenuItem(choice.name, {
                            close()

                            if (choice.id != current) {
                                runner { store.connectGroups(directedRequest(connection.sourceGroupId, connection.targetGroupId, choice.id)) }
                            }
                        }, checked = choice.id == current)
                    }

                    Hairline(Modifier.padding(vertical = Tokens.Space.S1))
                },
            ) {
                store.act { store.revokeGroupConnection(connection.id) }
            }
        }

        if (groups.size < 2) {
            Label("Create a second group in Settings, Groups to connect groups.", color = MyTheme.colors.textMuted)
        } else if (!connecting) {
            MyButton({
                runner.failure = null
                first = groups[0].id
                second = groups[1].id
                direction = DirectionBoth
                connecting = true
            }, text = "Connect groups", variant = ButtonVariant.Outline)
        } else {
            val choices = groups.map { Choice(it.id, it.name) }

            val names = groups.associate { it.id to it.name }

            runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }
            OptionMenu("First group", choices, first, { first = it })
            OptionMenu("Second group", choices, second, { second = it })
            OptionMenu("Direction", directionChoices(names[first].orEmpty(), names[second].orEmpty()), direction, { direction = it })
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                MyButton({
                    if (first == second) {
                        runner.failure = "Choose two different groups."
                    } else {
                        runner {
                            store.connectGroups(directedRequest(first, second, direction))
                            connecting = false
                        }
                    }
                }, text = "Connect", variant = ButtonVariant.Outline)
                MyButton({ connecting = false }, text = "Cancel", muted = true)
            }
        }
    }
}

private const val DirectionBoth = "both"

private const val DirectionForward = "forward"

private const val DirectionReverse = "reverse"

private fun directionChoices(first: String, second: String) = listOf(
    Choice(DirectionBoth, "Both ways"),
    Choice(DirectionForward, "Only $first may reach $second"),
    Choice(DirectionReverse, "Only $second may reach $first"),
)

private fun directedRequest(first: String, second: String, direction: String) = when (direction) {
    DirectionReverse -> CreateGroupConnectionRequest(second, first, bothWays = false)
    DirectionForward -> CreateGroupConnectionRequest(first, second, bothWays = false)
    else -> CreateGroupConnectionRequest(first, second, bothWays = true)
}

@Composable
private fun ConnectionRow(first: String, second: String, menuLabel: String, note: String? = null, menuExtra: (@Composable (close: () -> Unit) -> Unit)? = null, onRevoke: () -> Unit) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Column(Modifier.weight(1f)) {
            Label(first)
            Label(second, color = MyTheme.colors.textMuted)

            if (note != null) {
                Label(note, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
            }
        }

        RowMenu(menuLabel) { close ->
            menuExtra?.invoke(close)

            MenuItem("Revoke", {
                close()
                onRevoke()
            })
        }
    }

    Hairline()
}
