package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import mycelium.api.AccountPrice
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import mycelium.state.AccountOnNode
import mycelium.state.AgentAccount
import mycelium.state.Store
import mycelium.state.UnnamedAccount
import mycelium.state.agentName
import mycelium.state.expiryDay
import mycelium.state.instantOrNull
import mycelium.state.loginMethodName
import mycelium.state.priceLabel
import mycelium.state.usageProjection
import mycelium.state.windowName

@Composable
fun AgentAccountPage(store: Store, account: AgentAccount, nodes: List<NodeSummary>, price: AccountPrice?, onSignIn: (PendingSignIn) -> Unit, onPricesChanged: () -> Unit, onBack: () -> Unit) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val localDateTime: (Long) -> String? = { millis -> platform?.localDateTime(millis) }
    val named = account.label.takeIf { it != UnnamedAccount }
    val elsewhere = nodes.filter { node -> node.online && node.agents.any { it.agent == account.agent && it.available } && account.nodes.none { it.nodeId == node.id } }
    val managed = account.nodes.filter { it.managed }
    var pricing by remember { mutableStateOf(false) }
    var addingReset by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf(false) }
    var signingOut by remember { mutableStateOf<AccountOnNode?>(null) }
    val anotherNode = rememberRowMenuState()

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            BackButton("Accounts", onBack, showText = true)
            Label(account.label, Modifier.semantics { heading() }, Tokens.Type.Title)
            Label(agentName(account.agent), color = colors.textMuted)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Label("Signed in on", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)

                if (named != null && elsewhere.isNotEmpty()) {
                    Box {
                        MyButton({ anotherNode.open = true }, text = "Sign in on another node", icon = MyIcon.Plus, size = ButtonSize.Sm, variant = ButtonVariant.Outline, selected = anotherNode.open)

                        RowMenuPopup(anotherNode) { close ->
                            for (node in elsewhere) {
                                MenuItem(node.name, onClick = { close(); onSignIn(PendingSignIn(node, account.agent, named)) }, leading = nodeIcon(node))
                            }
                        }
                    }
                }
            }

            Table(
                listOf(Col("Node", 120.dp, weight = 1f), Col("State", 160.dp, preferred = 200.dp), Col("Method", 120.dp, priority = 1), ActionColumn),
                account.nodes.size,
                { row, column ->
                    val on = account.nodes[row]
                    val node = nodes.firstOrNull { it.id == on.nodeId }

                    when (column) {
                        0 -> TableCell.Text(on.nodeName)
                        1 -> when (on.state) {
                            LoginState.SignedIn -> TableCell.Slot("Signed in", extra = 24.dp) { StatusMark("Signed in", Tone.Done, MyIcon.Check) }
                            LoginState.Unknown -> TableCell.Text("Unknown", color = colors.textMuted)
                            LoginState.SignedOut, LoginState.Expired -> {
                                val text = if (on.state == LoginState.Expired) "Expired" else "Signed out"

                                TableCell.Slot("$text Sign in", extra = 48.dp) {
                                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                        StatusMark(text, Tone.Failed, MyIcon.Failed, Modifier.semantics { contentDescription = "$text on ${on.nodeName}" })

                                        if (node != null && node.online) {
                                            MyButton({ onSignIn(PendingSignIn(node, account.agent, named)) }, text = "Sign in", size = ButtonSize.Sm, variant = ButtonVariant.Outline, description = "Sign in on ${on.nodeName}")
                                        }
                                    }
                                }
                            }
                        }

                        2 -> TableCell.Text(loginMethodName(on.method), color = colors.textMuted)
                        else -> TableCell.Slot {
                            if (named != null && (on.managed || on.state == LoginState.SignedIn)) {
                                RowMenu(on.nodeName) { close ->
                                    if (on.state == LoginState.SignedIn) {
                                        MenuItem("Sign out on ${on.nodeName}", onClick = { close(); signingOut = on }, leading = MyIcon.Close)
                                    }

                                    if (on.managed) {
                                        MenuItem("Remove from ${on.nodeName}", onClick = { close(); store.act { store.api.removeLogin(on.nodeId, account.agent, named) } }, leading = MyIcon.Close)
                                    }
                                }
                            }
                        }
                    }
                },
            )
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Plan and price", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            account.plan?.let { plan -> Label("Plan: $plan") }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                if (price == null) {
                    Label("Price unknown", color = colors.textMuted)
                } else {
                    Label(priceLabel(price))
                    Tag("Your price")
                }

                MyButton({ pricing = true }, text = "Change price", size = ButtonSize.Sm, muted = true, description = "Change the price of ${account.label}")
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Label("Usage", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
                MyButton({ store.openScreen("usage") }, text = "Usage history", size = ButtonSize.Sm, muted = true)
            }

            if (account.windows.isEmpty()) {
                Label(account.usageNote ?: "Usage unknown", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            for (window in account.windows) {
                val percent = window.usedPercent.roundToInt().coerceIn(0, 100)
                val name = windowName(window.name).replaceFirstChar { it.uppercase() }
                val projection = usageProjection(window, localDateTime)

                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label(name, Modifier.width(UsageNameWidth), Tokens.Type.Meta, maxLines = 1)
                        Box(Modifier.widthIn(max = UsageBarWidth).weight(1f)) { Meter(percent, "$name $percent% used") }
                        Label("$percent% used", Modifier.width(UsageNameWidth), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                    }

                    if (projection.isNotEmpty()) {
                        Label(projection, style = Tokens.Type.Meta, color = colors.textMuted)
                    }
                }
            }
        }

        if (account.agent == "claude") {
            val recorded = account.resetCredits?.credits.orEmpty().filter { it.manual }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Label("Limit resets", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
                    MyButton({ addingReset = true }, text = "Add a reset", icon = MyIcon.Plus, size = ButtonSize.Sm, muted = true)
                }

                if (recorded.isEmpty()) {
                    Label("No limit resets recorded", style = Tokens.Type.Meta, color = colors.textMuted)
                }

                for (credit in recorded) {
                    val expires = credit.expiresAt?.let(::instantOrNull)?.let { expiryDay(it, localDateTime) }.orEmpty()

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
                        Label(credit.title.orEmpty(), Modifier.width(UsageNameWidth), Tokens.Type.Meta, maxLines = 1)
                        Label("Expires $expires", Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                        MyButton({ store.act { store.api.removeManualReset(credit.id) } }, text = "Remove", description = "Remove the ${credit.title}, expires $expires", size = ButtonSize.Sm, muted = true)
                    }
                }
            }
        }

        if (managed.isNotEmpty() && named != null) {
            Row {
                MyButton({ removing = true }, text = "Remove this account", icon = MyIcon.Close, variant = ButtonVariant.Outline)
            }
        }
    }

    if (pricing) {
        PriceDialog(store, account.agent, account.label, price, listPrice = null, onSaved = onPricesChanged, onDismiss = { pricing = false })
    }

    if (addingReset) {
        AddResetDialog(store, account.agent, account.label, onDismiss = { addingReset = false })
    }

    signingOut?.let { on ->
        if (named != null) {
            ConfirmDialog(
                "Sign ${account.label} out on ${on.nodeName}?",
                "Chats on ${on.nodeName} that use it stop until it is signed in again.",
                "Sign out",
                { signingOut = null },
            ) {
                signingOut = null
                store.act { store.api.signOutLogin(on.nodeId, account.agent, named) }
            }
        }
    }

    if (removing && named != null) {
        ConfirmDialog(
            "Remove ${account.label}?",
            "It is signed out and removed from ${managed.joinToString(", ") { it.nodeName }}. Chats that use it stop until they move to another account.",
            "Remove ${account.label}",
            { removing = false },
        ) {
            removing = false
            store.act { managed.forEach { store.api.removeLogin(it.nodeId, account.agent, named) } }
            onBack()
        }
    }
}

private val UsageNameWidth = 130.dp
private val UsageBarWidth = 240.dp
