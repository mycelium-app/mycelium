package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.Platform
import mycelium.api.AgentInfo
import mycelium.api.LoginInfo
import mycelium.api.LoginState
import mycelium.api.NodeSummary
import mycelium.api.UpdateNodeRequest
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.agentVersion
import mycelium.state.isLaptop
import mycelium.state.loginMethodName

data class LoginSession(val loginId: String, val agent: String)

fun loginMethodFor(agent: String): String = when (agent) {
    "codex" -> "device_auth"
    else -> "oauth_token"
}

fun firstUrl(text: String): String? = Regex("https?://[^\\s\"'<>]+").find(text)?.value

fun withoutTerminalCodes(text: String): String =
    Regex("\u001B(\\[[0-?]*[ -/]*[@-~]|\\][^\u0007\u001B]*(\u0007|\u001B\\\\)|[@-Z\\\\-_])").replace(text, "").replace("\r", "")

fun oneTimeCode(text: String): String? = Regex("\\b[A-Z0-9]{4}-[A-Z0-9]{4,}\\b").find(text)?.value

private val JoinMethods = listOf("Shell", "Nix", "NixOS or nix-darwin")

fun joinCommands(method: String, controlPlane: String): List<Pair<String, String>> = when (method) {
    "Shell" -> listOf("Install" to "curl -fsSL $controlPlane/install.sh | sh", "Join" to "mycelium node join $controlPlane")
    "Nix" -> listOf("Join" to "nix run github:mycelium-app/mycelium -- node join $controlPlane")
    else -> listOf(
        "Configuration" to "services.mycelium-node = {\n  enable = true;\n  controlPlane = \"$controlPlane\";\n};",
        "Join" to "sudo mycelium node join",
    )
}

@Composable
fun NodesScreen(store: Store, state: AppState, platform: Platform, onOpenNode: (String) -> Unit = {}) {
    val api = store.api
    val sessions = remember { mutableStateMapOf<String, LoginSession>() }
    var joinOpen by remember { mutableStateOf(false) }
    var revoking by remember { mutableStateOf<String?>(null) }
    val nodes = state.nodes.values.sortedWith(compareByDescending<NodeSummary> { it.online }.thenBy { it.name })
    val touch = LocalTouch.current

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Nodes")
            Box(Modifier.weight(1f))

            Box {
                MyButton({ joinOpen = true }, text = if (touch) null else "Join a node", icon = MyIcon.Plus, variant = ButtonVariant.Outline, description = "Join a node")

                if (joinOpen) {
                    JoinPopup(api.baseUrl, platform, onDismiss = { joinOpen = false })
                }
            }
        }

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar()) {
            if (nodes.isEmpty()) {
                Label("No node has joined yet. Select Join a node to connect the first one.", Modifier.padding(Tokens.Space.S4), color = MyTheme.colors.textMuted)
            }

            if (nodes.isNotEmpty() && !touch) {
                NodeColumnHeads()
            }

            for (node in nodes) {
                NodeGroup(
                    node = node,
                    session = sessions[node.id],
                    output = sessions[node.id]?.let { state.loginOutput[it.loginId].orEmpty() }.orEmpty(),
                    finished = sessions[node.id]?.let { state.finishedLogins[it.loginId] },
                    confirmingRevoke = revoking == node.id,
                    platform = platform,
                    onOpen = { onOpenNode(node.id) },
                    onLogin = { agent, account ->
                        store.act {
                            val started = api.startLogin(node.id, agent, loginMethodFor(agent), account)
                            sessions[node.id] = LoginSession(started.loginId, agent)
                        }
                    },
                    onLoginInput = { text -> sessions[node.id]?.let { s -> store.act { api.loginInput(s.loginId, text) } } },
                    onLoginClose = {
                        sessions[node.id]?.let { s ->
                            store.act { api.stopLogin(s.loginId) }
                            sessions.remove(node.id)
                        }
                    },
                    onDrain = { store.act { api.drainNode(node.id, !node.draining) } },
                    onRename = { name -> store.act { api.updateNode(node.id, UpdateNodeRequest(name = name)) } },
                    onIcon = { icon -> store.act { api.updateNode(node.id, UpdateNodeRequest(icon = icon)) } },
                    onColour = { colour -> store.act { api.updateNode(node.id, UpdateNodeRequest(colour = colour)) } },
                    onAskRevoke = { revoking = node.id },
                    onCancelRevoke = { revoking = null },
                    onRevoke = {
                        revoking = null
                        store.act { api.revokeNode(node.id) }
                    },
                )
            }
        }
    }
}

@Composable
private fun JoinPopup(controlPlane: String, platform: Platform, onDismiss: () -> Unit) {
    var method by remember { mutableStateOf(JoinMethods.first()) }

    MenuPopup(onDismiss = onDismiss, width = Tokens.Size.Popover) {
        Column(Modifier.padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Label("Join a node", Modifier.semantics { heading() }, Tokens.Type.Title)
            Segmented(JoinMethods, method, { method = it }, groupLabel = "How the machine runs Mycelium")

            for ((label, command) in joinCommands(method, controlPlane)) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label(label, style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                    CopyableCode(command, label, platform::copyToClipboard)
                }
            }

            StatusMark("Waiting for the next node", Tone.Working, MyIcon.Working)
        }
    }
}

@Composable
private fun NodeGroup(
    node: NodeSummary,
    session: LoginSession?,
    output: String,
    finished: Boolean?,
    confirmingRevoke: Boolean,
    platform: Platform,
    onOpen: () -> Unit,
    onLogin: (String, String?) -> Unit,
    onLoginInput: (String) -> Unit,
    onLoginClose: () -> Unit,
    onDrain: () -> Unit,
    onRename: (String) -> Unit,
    onIcon: (String) -> Unit,
    onColour: (String) -> Unit,
    onAskRevoke: () -> Unit,
    onCancelRevoke: () -> Unit,
    onRevoke: () -> Unit,
) {
    val colors = MyTheme.colors
    var addOpen by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf<String?>(null) }
    val installed = node.agents.filter { it.available }
    val signedOut = installed.filter { agent -> node.logins.none { it.agent == agent.agent && it.state == LoginState.SignedIn } }
    val touch = LocalTouch.current

    Column(Modifier.fillMaxWidth()) {
        Row(
            Modifier.fillMaxWidth().background(colors.bgPanel).heightIn(min = rowHeight()).padding(start = Tokens.Space.S4, end = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(nodeIcon(node), nodeColour(node), Tokens.Size.IconSm)
            val draft = renaming

            if (draft != null) {
                fun save() {
                    onRename(draft.trim())
                    renaming = null
                }

                TextInput(draft, { renaming = it }, label = "Name for ${node.name}", showLabel = false, modifier = Modifier.width(Tokens.Size.FormColumn), onSubmit = ::save)
                MyButton(::save, text = "Save", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                MyButton({ renaming = null }, text = "Cancel", size = ButtonSize.Sm, muted = true)
            } else {
                Label(node.name, Modifier.semantics { heading() }.pressable(onOpen, description = "Open ${node.name}", shape = ControlShape).padding(horizontal = Tokens.Space.S1), Tokens.Type.UiStrong, maxLines = 1)
            }

            when {
                node.draining -> StatusMark("Draining", Tone.Attention, MyIcon.Clock)
                !node.online -> StatusMark("Offline", Tone.Muted, MyIcon.Failed)
                else -> Label("Online", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            if (touch) {
                Box(Modifier.weight(1f))
            } else {
                Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    Label("${node.os} ${node.arch}  ·  ${node.version}", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                    node.updateAvailable?.let { UpdateBadge(it) }
                }
            }

            if (signedOut.isNotEmpty()) {
                Box {
                    MyButton({ addOpen = true }, text = if (touch) null else "Add login", icon = MyIcon.Plus, size = ButtonSize.Sm, muted = true, description = "Add login on ${node.name}")

                    if (addOpen) {
                        MenuPopup(onDismiss = { addOpen = false }) {
                            MenuLabel("Sign in an agent on ${node.name}")

                            for (agent in signedOut) {
                                MenuItem(agentName(agent.agent), onClick = { addOpen = false; onLogin(agent.agent, null) })
                            }
                        }
                    }
                }
            }

            RowMenu(node.name) { close ->
                MenuItem("Rename", onClick = { close(); renaming = node.name }, leading = MyIcon.Pencil)
                MenuLabel("Icon")
                Row(Modifier.padding(horizontal = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                    for ((key, icon) in NodeIcons) {
                        MyButton({ close(); onIcon(key) }, icon = icon, description = "Use the ${key} icon", muted = true, selected = nodeIcon(node) == icon)
                    }
                }
                MenuLabel("Colour")
                Row(Modifier.padding(horizontal = Tokens.Space.S1), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                    val current = nodeColour(node)

                    for (accent in NodeColours) {
                        val colour = if (LocalAppearance.current.dark) accent.dark else accent.light
                        val name = accent.name.lowercase()

                        Box(
                            Modifier
                                .size(touchMin(Tokens.Size.ControlMd))
                                .pressable({ close(); onColour(name) }, description = "Use the $name colour", shape = ControlShape, selected = colour == current, stateDescription = if (colour == current) "Chosen" else null),
                            contentAlignment = Alignment.Center,
                        ) {
                            Box(Modifier.size(Tokens.Size.IconMd).background(colour, CircleShape))
                        }
                    }
                }
                MenuItem(if (node.draining) "Resume new conversations" else "Drain", onClick = { close(); onDrain() })
                MenuItem("Revoke", onClick = { close(); onAskRevoke() })
            }
        }

        Hairline()

        if (confirmingRevoke) {
            Row(
                Modifier.fillMaxWidth().padding(start = Tokens.Space.S4, end = Tokens.Space.S2).heightIn(min = rowHeight()),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Label("Revoke ${node.name}? It disconnects now and must join again to come back.", Modifier.weight(1f), color = colors.statusFailed)
                MyButton(onRevoke, text = "Revoke ${node.name}", variant = ButtonVariant.Danger, size = ButtonSize.Sm)
                MyButton(onCancelRevoke, text = "Cancel", size = ButtonSize.Sm, muted = true)
            }

            Hairline()
        }

        if (installed.isEmpty()) {
            Label("No agents installed", Modifier.padding(start = NameIndent, top = Tokens.Space.S2, bottom = Tokens.Space.S2), color = colors.textMuted)
        }

        for (agent in installed) {
            val accounts = node.logins.filter { it.agent == agent.agent }

            for (login in accounts.ifEmpty { listOf(null) }) {
                AgentLoginRow(agent, login, onLogin = { onLogin(agent.agent, login?.account) })
                Hairline()
            }
        }

        if (session != null) {
            LoginPanel(session, output, finished, platform::openUrl, platform::copyToClipboard, onLoginInput, onLoginClose)
            Hairline()
        }
    }
}

private val NameIndent = Tokens.Space.S4 + Tokens.Size.IconSm + Tokens.Space.S2

@Composable
private fun NodeColumnHeads() {
    val colors = MyTheme.colors

    Row(
        Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(start = NameIndent, end = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label("Agent", Modifier.weight(1.4f), Tokens.Type.MetaStrong, colors.textMuted)
        Label("Account", Modifier.weight(1.2f), Tokens.Type.MetaStrong, colors.textMuted)
        Label("Method", Modifier.weight(1f), Tokens.Type.MetaStrong, colors.textMuted)
        Label("State", Modifier.weight(1.2f), Tokens.Type.MetaStrong, colors.textMuted)
    }

    Hairline()
}

@Composable
private fun AgentLoginRow(agent: AgentInfo, login: LoginInfo?, onLogin: () -> Unit) {
    val colors = MyTheme.colors
    val touch = LocalTouch.current

    Row(
        Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(start = NameIndent, end = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (touch) {
            Column(Modifier.weight(1f).padding(vertical = Tokens.Space.S1_5)) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                    Label(agentName(agent.agent), maxLines = 1)
                    Label(agentVersion(agent.version), color = colors.textMuted, maxLines = 1)
                    agent.updateAvailable?.let { UpdateBadge(it) }
                }

                val detail = listOfNotNull(login?.account?.ifEmpty { null }, login?.let { loginMethodName(it.method) }).joinToString("  ·  ")

                if (detail.isNotEmpty()) {
                    Label(detail, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }
            }
        } else {
            Row(Modifier.weight(1.4f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                Label(agentName(agent.agent), maxLines = 1)
                Label(agentVersion(agent.version), color = colors.textMuted, maxLines = 1)
                agent.updateAvailable?.let { UpdateBadge(it) }
            }

            Label(login?.account.orEmpty(), Modifier.weight(1.2f), color = colors.text, maxLines = 1)
            Label(login?.let { loginMethodName(it.method) }.orEmpty(), Modifier.weight(1f), color = colors.textMuted, maxLines = 1)
        }

        Row(if (touch) Modifier else Modifier.weight(1.2f), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            when (login?.state) {
                LoginState.SignedIn -> Label("Signed in", color = colors.textMuted)
                LoginState.Expired -> StatusMark("Expired", Tone.Failed, MyIcon.Failed)
                LoginState.SignedOut, null -> StatusMark("Signed out", Tone.Failed, MyIcon.Failed)
                LoginState.Unknown -> Label("Unknown", color = colors.textMuted)
            }

            if (login?.state != LoginState.SignedIn) {
                MyButton(onLogin, text = if (login == null) "Log in" else "Log in again", size = ButtonSize.Sm, variant = ButtonVariant.Outline, description = "Log in ${agentName(agent.agent)}")
            }
        }
    }
}

@Composable
fun LoginPanel(
    session: LoginSession,
    output: String,
    finished: Boolean?,
    onOpenUrl: (String) -> Unit,
    onCopy: (String) -> Unit,
    onInput: (String) -> Unit,
    onClose: () -> Unit,
) {
    val colors = MyTheme.colors
    var reply by remember { mutableStateOf("") }
    val text = withoutTerminalCodes(output)
    val link = firstUrl(text)
    val code = oneTimeCode(text)
    val send = {
        onInput(reply + "\r")
        reply = ""
    }

    Column(
        Modifier.fillMaxWidth().background(colors.bgSelected).padding(Tokens.Space.S3).semantics { contentDescription = "${agentName(session.agent)} sign-in" },
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
    ) {
        Label("Sign in ${agentName(session.agent)}", style = Tokens.Type.UiStrong)

        SelectionContainer {
            Box(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = rowHeight(), max = Tokens.Size.Popover / 2)
                    .background(colors.bgPanel, ControlShape)
                    .verticalScrollWithBar()
                    .padding(Tokens.Space.S2),
            ) {
                Label(text.ifEmpty { "Waiting for the node to start the sign-in." }, style = Tokens.Type.Code)
            }
        }

        if (link != null && finished == null) {
            MyButton({ code?.let(onCopy); onOpenUrl(link) }, text = if (code == null) "Open the sign-in page" else "Copy the code and open the sign-in page", variant = ButtonVariant.Primary, size = ButtonSize.Lg)
        }

        when (finished) {
            true -> StatusMark("Signed in", Tone.Muted, MyIcon.Check)
            false -> StatusMark("Sign-in failed. Select Log in to try again.", Tone.Failed, MyIcon.Failed)
            null -> {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
                    TextInput(
                        reply,
                        { reply = it },
                        label = "Reply to the sign-in",
                        placeholder = "Paste the code, then send",
                        modifier = Modifier.weight(1f),
                        mono = true,
                        onSubmit = send,
                    )
                    MyButton(send, text = "Send", variant = ButtonVariant.Outline)
                }

                StatusMark("Waiting for you to sign in", Tone.Working, MyIcon.Working)
            }
        }

        Row(horizontalArrangement = Arrangement.End, modifier = Modifier.fillMaxWidth()) {
            MyButton(onClose, text = if (finished == null) "Cancel" else "Close", size = ButtonSize.Sm, muted = true)
        }
    }
}
