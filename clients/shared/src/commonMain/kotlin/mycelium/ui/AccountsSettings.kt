package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
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
import mycelium.api.LoginState
import mycelium.state.AgentAccount
import mycelium.state.usageProjection
import mycelium.state.windowName
import mycelium.state.Store
import mycelium.state.agentAccounts
import mycelium.state.agentName
import mycelium.state.loginMethodName
import mycelium.state.newAccountName

@Composable
fun AccountsSettings(store: Store) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    val accounts = agentAccounts(state.nodes.values)
    val agents = (accounts.map { it.agent } + state.nodes.values.flatMap { node -> node.agents.filter { it.available }.map { it.agent } }).distinct()
    var signIn by remember { mutableStateOf<LoginSession?>(null) }
    var menuFor by remember { mutableStateOf<String?>(null) }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        if (accounts.isEmpty()) {
            Label("No agent account is signed in on any node. Add a login from a node on the Nodes page.", color = colors.textMuted)
        }

        for (agent in agents) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label(agentName(agent), Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                ContentTable(listOf(ColumnSpec(flexible = true), ColumnSpec(max = 360.dp), ColumnSpec(max = 200.dp))) {
                    ContentRow(header = true) {
                        Label("Account", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                        Label("Signed in on", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                        Label("Method", style = Tokens.Type.MetaStrong, color = colors.textMuted)
                    }

                    Hairline()

                    for (account in accounts.filter { it.agent == agent }) {
                        ContentRow(Modifier.padding(vertical = Tokens.Space.S1)) {
                            Label(account.label, maxLines = 1)

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

                            Label(loginMethodName(account.method), color = colors.textMuted)
                        }

                        AccountUsage(account)

                        Hairline()
                    }
                }

                val signInNodes = state.nodes.values.filter { node -> node.online && node.agents.any { it.agent == agent && it.available } }.sortedBy { it.name }

                if (signInNodes.isNotEmpty()) {
                    Box {
                        MyButton({ menuFor = agent }, text = "Sign in on a node", size = ButtonSize.Sm, variant = ButtonVariant.Outline, description = "Sign in ${agentName(agent)} on a node")

                        if (menuFor == agent) {
                            MenuPopup(onDismiss = { menuFor = null }) {
                                MenuLabel("Sign in ${agentName(agent)} on")

                                for (node in signInNodes) {
                                    MenuItem(
                                        node.name,
                                        onClick = {
                                            menuFor = null
                                            store.act {
                                                val started = store.api.startLogin(node.id, agent, loginMethodFor(agent), newAccountName())
                                                signIn = LoginSession(started.loginId, agent)
                                            }
                                        },
                                        leading = nodeIcon(node),
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    signIn?.let { session -> AccountSignInDialog(store, session, onClose = { signIn = null }) }
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
