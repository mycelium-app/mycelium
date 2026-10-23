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
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.api.LoginState
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

                Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    Label("Account", Modifier.weight(2f), Tokens.Type.MetaStrong, colors.textMuted)
                    Label("Signed in on", Modifier.weight(3f), Tokens.Type.MetaStrong, colors.textMuted)
                    Label("Method", Modifier.weight(1.5f), Tokens.Type.MetaStrong, colors.textMuted)
                }

                Hairline()

                for (account in accounts.filter { it.agent == agent }) {
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Label(account.label, Modifier.weight(2f), maxLines = 1)

                        FlowRow(Modifier.weight(3f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                            for (node in account.nodes) {
                                when (node.state) {
                                    LoginState.SignedIn -> Label(node.nodeName)
                                    LoginState.Expired -> StatusMark("${node.nodeName}: expired", Tone.Failed, MyIcon.Failed)
                                    LoginState.SignedOut -> StatusMark("${node.nodeName}: signed out", Tone.Failed, MyIcon.Failed)
                                    LoginState.Unknown -> Label("${node.nodeName}: unknown", color = colors.textMuted)
                                }
                            }
                        }

                        Label(loginMethodName(account.method), Modifier.weight(1.5f), color = colors.textMuted)
                    }

                    Hairline()
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
