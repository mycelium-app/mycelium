package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlin.math.roundToInt
import androidx.compose.ui.unit.dp
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.AccountPrice
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import mycelium.state.AgentAccount
import mycelium.state.usageProjection
import mycelium.state.windowName
import mycelium.state.Store
import mycelium.state.agentAccounts
import mycelium.state.agentName
import mycelium.state.loginMethodName
import mycelium.state.newAccountName
import mycelium.state.priceLabel
import mycelium.state.splitSignedOut

@Composable
fun AccountsSettings(store: Store) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    val accounts = agentAccounts(state.nodes.values)
    val agents = (accounts.map { it.agent } + state.nodes.values.flatMap { node -> node.agents.filter { it.available }.map { it.agent } }).distinct()
    var signIn by remember { mutableStateOf<LoginSession?>(null) }
    var prices by remember { mutableStateOf(emptyList<AccountPrice>()) }
    var pricing by remember { mutableStateOf<AgentAccount?>(null) }
    val startSignIn: (NodeSummary, String) -> Unit = { node, agent ->
        store.act {
            val started = store.api.startLogin(node.id, agent, loginMethodFor(agent), newAccountName())
            signIn = LoginSession(started.loginId, agent)
        }
    }

    LaunchedEffect(Unit) {
        prices = store.accountPrices().orEmpty()
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        if (agents.isEmpty()) {
            Label("No agent account is signed in on any node. Add a login from a node on the Nodes page.", color = colors.textMuted)
        }

        for (agent in agents) {
            val split = splitSignedOut(accounts.filter { it.agent == agent })
            val signInNodes = state.nodes.values.filter { node -> node.online && node.agents.any { it.agent == agent && it.available } }.sortedBy { it.name }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Label(agentName(agent), Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)

                    if (signInNodes.isNotEmpty()) {
                        RowMenu(agentName(agent)) { close ->
                            for (node in signInNodes) {
                                MenuItem("Sign in on ${node.name}", onClick = { close(); startSignIn(node, agent) }, leading = nodeIcon(node))
                            }
                        }
                    }
                }

                if (split.accounts.isNotEmpty()) {
                    val accounts = split.accounts

                    Table(
                        listOf(
                            Col("Account", 120.dp, weight = 1f),
                            Col("Signed in on", 140.dp, max = 360.dp, overflow = Overflow.Wrap),
                            Col("Method", 88.dp, priority = 1),
                            Col("Price", 100.dp, preferred = 200.dp),
                            ActionColumn,
                        ),
                        accounts.size,
                        { row, column ->
                            val account = accounts[row]

                            when (column) {
                                0 -> TableCell.Text(account.label)
                                1 -> TableCell.Slot(account.nodes.joinToString(" ") { it.nodeName }) {
                                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                                        for (node in account.nodes) {
                                            when (node.state) {
                                                LoginState.SignedIn -> Label(node.nodeName)
                                                LoginState.Expired -> StatusMark("${node.nodeName}: expired", Tone.Failed, MyIcon.Failed)
                                                LoginState.SignedOut -> StatusMark("${node.nodeName}: signed out", Tone.Failed, MyIcon.Failed)
                                                LoginState.Unknown -> Label("${node.nodeName}: unknown", color = colors.textMuted)
                                            }
                                        }
                                    }
                                }

                                2 -> TableCell.Text(loginMethodName(account.method), color = colors.textMuted)

                                3 -> {
                                    val price = prices.firstOrNull { it.agent == account.agent && it.account == account.label }

                                    if (price == null) {
                                        TableCell.Text("Unknown", color = colors.textMuted)
                                    } else {
                                        TableCell.Slot(priceLabel(price), extra = 90.dp) {
                                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                                Label(priceLabel(price))
                                                Tag("Your price")
                                            }
                                        }
                                    }
                                }

                                else -> TableCell.Slot {
                                    RowMenu(account.label) { close ->
                                        MenuItem("Set your price", onClick = { close(); pricing = account }, leading = MyIcon.Pencil)
                                    }
                                }
                            }
                        },
                        detail = { row -> AccountUsage(accounts[row]) },
                    )
                }

                if (split.accounts.isEmpty()) {
                    Label("No account signed in", color = colors.textMuted)
                }
            }
        }
    }

    signIn?.let { session -> AccountSignInDialog(store, session, onClose = { signIn = null }) }

    pricing?.let { account ->
        PriceDialog(
            store,
            account.agent,
            account.label,
            prices.firstOrNull { it.agent == account.agent && it.account == account.label },
            listPrice = null,
            onSaved = { store.act { prices = store.accountPrices().orEmpty() } },
            onDismiss = { pricing = null },
        )
    }
}

@Composable
private fun AccountUsage(account: AgentAccount) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val localDateTime: (Long) -> String? = { millis -> platform?.localDateTime(millis) }

    Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        if (account.windows.isEmpty()) {
            Label("Usage unknown", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        for (window in account.windows) {
            val percent = window.usedPercent.roundToInt().coerceIn(0, 100)
            val name = windowName(window.name).replaceFirstChar { it.uppercase() }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                    Label(name, Modifier.width(UsageNameWidth), Tokens.Type.Meta, maxLines = 1)
                    Box(Modifier.widthIn(max = UsageBarWidth).weight(1f)) { Meter(percent, "$name $percent% used") }
                    Label("$percent% used", Modifier.width(UsageNameWidth), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                }

                val projection = usageProjection(window, localDateTime)

                if (projection.isNotEmpty()) {
                    Label(projection, style = Tokens.Type.Meta, color = colors.textMuted)
                }
            }
        }
    }
}

private val UsageNameWidth = 130.dp
private val UsageBarWidth = 240.dp
