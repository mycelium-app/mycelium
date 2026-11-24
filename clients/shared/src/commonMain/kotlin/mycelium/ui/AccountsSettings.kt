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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import mycelium.api.AccountPrice
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import mycelium.state.AgentAccount
import mycelium.state.Store
import mycelium.state.UnnamedAccount
import mycelium.state.agentAccounts
import mycelium.state.agentName
import mycelium.state.newAccountName
import mycelium.state.splitSignedOut

data class PendingSignIn(val node: NodeSummary, val agent: String, val account: String?)

@Composable
fun AccountsSettings(store: Store) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    val accounts = agentAccounts(state.nodes.values)
    val nodes = state.nodes.values.sortedBy { it.name }
    val agents = (accounts.map { it.agent } + nodes.flatMap { node -> node.agents.filter { it.available }.map { it.agent } }).distinct()
    val listed = agents.flatMap { agent -> splitSignedOut(accounts.filter { it.agent == agent }).accounts }
    var signIn by remember { mutableStateOf<LoginSession?>(null) }
    var queue by remember { mutableStateOf(emptyList<PendingSignIn>()) }
    var prices by remember { mutableStateOf(emptyList<AccountPrice>()) }
    var opened by remember { mutableStateOf<Pair<String, String>?>(null) }
    val startSignIn: (PendingSignIn) -> Unit = { pending ->
        store.act {
            val started = store.api.startLogin(pending.node.id, pending.agent, loginMethodFor(pending.agent), pending.account)
            signIn = LoginSession(started.loginId, pending.agent)
        }
    }
    val signInInTurn: (List<PendingSignIn>) -> Unit = { pending ->
        queue = pending.drop(1)
        pending.firstOrNull()?.let(startSignIn)
    }
    val needYou = listed.flatMap { account ->
        account.nodes
            .filter { it.state == LoginState.SignedOut || it.state == LoginState.Expired }
            .mapNotNull { on -> nodes.firstOrNull { it.id == on.nodeId && it.online } }
            .map { PendingSignIn(it, account.agent, account.label) }
    }

    LaunchedEffect(Unit) {
        prices = store.accountPrices().orEmpty()
    }

    val shown = opened?.let { (agent, label) -> listed.firstOrNull { it.agent == agent && it.label == label } }

    if (shown != null) {
        AgentAccountPage(
            store,
            shown,
            nodes,
            prices.firstOrNull { it.agent == shown.agent && it.account == shown.label },
            onSignIn = startSignIn,
            onPricesChanged = { store.act { prices = store.accountPrices().orEmpty() } },
            onBack = { opened = null },
        )
    } else {
        Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            if (needYou.isNotEmpty()) {
                val count = needYou.size

                Row(
                    Modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Polite },
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm)

                    Column(Modifier.weight(1f)) {
                        Label(if (count == 1) "1 sign-in needs you" else "$count sign-ins need you", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                        Label("Chats on these stop until they are signed in again.", style = Tokens.Type.Meta, color = colors.textMuted)
                    }

                    MyButton({ signInInTurn(needYou) }, text = "Sign in to all $count", variant = ButtonVariant.Primary)
                }
            }

            if (agents.isEmpty()) {
                Label("No agent is installed on any node. Join a node from Settings, Nodes.", color = colors.textMuted)
            } else {
                val rows = agents.flatMap { agent -> listOf(agent to null) + listed.filter { it.agent == agent }.map { agent to it } }

                Table(
                    listOf(Col("Account", 160.dp, weight = 1f)) + nodes.map { Col(it.name, 120.dp, preferred = 150.dp) } + listOf(Col("Usage", 90.dp), ActionColumn),
                    rows.size,
                    { row, column ->
                        val account = rows[row].second

                        when {
                            account == null -> TableCell.Text("")
                            column == 0 -> TableCell.Text(account.label)
                            column <= nodes.size -> accountCell(account, nodes[column - 1], colors.textMuted, startSignIn)
                            column == nodes.size + 1 -> TableCell.Text(if (account.windows.isEmpty()) "Unknown" else account.windows.take(2).joinToString(" · ") { "${it.usedPercent.roundToInt().coerceIn(0, 100)}%" }, color = colors.textMuted)
                            else -> TableCell.Slot {
                                RowMenu(account.label) { close ->
                                    MenuItem("Open", onClick = { close(); opened = account.agent to account.label }, leading = MyIcon.ChevronRight)
                                }
                            }
                        }
                    },
                    rowKey = { row -> "${rows[row].first}/${rows[row].second?.label}" },
                    onRow = { row -> rows[row].second?.let { opened = it.agent to it.label } },
                    rowDescription = { row -> rows[row].second?.let { "${agentName(it.agent)} ${it.label}" } ?: agentName(rows[row].first) },
                    rowSpan = { row ->
                        val agent = rows[row].first

                        if (rows[row].second == null) {
                            {
                                val installedOn = nodes.filter { node -> node.online && node.agents.any { it.agent == agent && it.available } }

                                Row(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                    Icon(agentIcon(agent), colors.textMuted, Tokens.Size.IconSm)
                                    Label(agentName(agent), Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)

                                    if (installedOn.isNotEmpty()) {
                                        RowMenu(agentName(agent)) { close ->
                                            for (node in installedOn) {
                                                MenuItem("Sign in a new account on ${node.name}", onClick = { close(); startSignIn(PendingSignIn(node, agent, newAccountName())) }, leading = nodeIcon(node))
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            null
                        }
                    },
                )

                Label("Select an account for its plan, price, limit resets and usage history.", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }
    }

    signIn?.let { session ->
        AccountSignInDialog(store, session, onClose = {
            signIn = null
            signInInTurn(queue)
        })
    }
}

private fun accountCell(account: AgentAccount, node: NodeSummary, muted: Color, onSignIn: (PendingSignIn) -> Unit): TableCell {
    val on = account.nodes.firstOrNull { it.nodeId == node.id }
    val installed = node.agents.any { it.agent == account.agent && it.available }
    val pending = PendingSignIn(node, account.agent, account.label.takeIf { it != UnnamedAccount })

    return when {
        on == null && installed && node.online -> TableCell.Slot("Add", extra = 24.dp) {
            MyButton({ onSignIn(pending) }, text = "Add", size = ButtonSize.Sm, muted = true, description = "Add ${account.label} on ${node.name}")
        }

        on == null && installed -> TableCell.Text("Not signed in", Tokens.Type.Meta, muted)
        on == null -> TableCell.Text("Not installed", Tokens.Type.Meta, muted)
        on.state == LoginState.SignedIn -> TableCell.Slot("Signed in", extra = 24.dp) { StatusMark("Signed in", Tone.Done, MyIcon.Check) }
        on.state == LoginState.Unknown -> TableCell.Text("Unknown", color = muted)
        else -> {
            val text = if (on.state == LoginState.Expired) "Expired" else "Signed out"

            TableCell.Slot("$text Sign in", extra = 48.dp) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    StatusMark(text, Tone.Failed, MyIcon.Failed, Modifier.semantics { contentDescription = "${account.label}: $text on ${node.name}" })

                    if (node.online) {
                        MyButton({ onSignIn(pending) }, text = "Sign in", size = ButtonSize.Sm, variant = ButtonVariant.Outline, description = "Sign in ${account.label} on ${node.name}")
                    }
                }
            }
        }
    }
}
