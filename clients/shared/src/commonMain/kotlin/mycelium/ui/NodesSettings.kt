package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import mycelium.Platform
import mycelium.api.NodeSummary
import mycelium.state.ColumnAlign
import mycelium.state.Store
import mycelium.state.agentName
import mycelium.state.agentVersion
import androidx.compose.foundation.layout.widthIn
import androidx.compose.ui.unit.dp
import androidx.compose.ui.semantics.clearAndSetSemantics
import mycelium.state.HarnessAgents

data class LoginSession(val loginId: String, val agent: String)

fun loginMethodFor(agent: String): String = when (agent) {
    "codex" -> "device_auth"
    "vibe" -> "browser"
    else -> "oauth_token"
}

fun firstUrl(text: String): String? = Regex("https?://[^\\s\"'<>]+").find(text)?.value

fun withoutTerminalCodes(text: String): String =
    Regex("\u001B(\\[[0-?]*[ -/]*[@-~]|\\][^\u0007\u001B]*(\u0007|\u001B\\\\)|[@-Z\\\\-_])").replace(text, "").replace("\r", "")

fun oneTimeCode(text: String): String? = Regex("\\b[A-Z0-9]{4}-[A-Z0-9]{4,}\\b").find(text)?.value

private val JoinMethods = listOf("Shell", "Nix", "NixOS or nix-darwin")
private const val KubernetesMethod = "Kubernetes"

private class JoinStep(val title: String, val note: String?, val commands: List<Pair<String, String>>)

private val DnsLabel = Regex("[a-z0-9]([-a-z0-9]*[a-z0-9])?")
private val DnsSubdomain = Regex("[a-z0-9]([-a-z0-9.]*[a-z0-9])?")
private const val MaxOutpostName = 40
private const val MaxNamespaceName = 63

private fun outpostNameError(name: String) =
    if (name.isNotEmpty() && !(name.length <= MaxOutpostName && DnsLabel.matches(name))) "Enter a name of up to $MaxOutpostName lowercase letters, numbers and hyphens, starting and ending with a letter or number." else null

private fun outpostNamespaceError(namespace: String) =
    if (namespace.isNotEmpty() && !(namespace.length <= MaxNamespaceName && DnsLabel.matches(namespace))) "Enter a namespace of up to $MaxNamespaceName lowercase letters, numbers and hyphens, starting and ending with a letter or number." else null

private fun outpostStorageClassError(storageClass: String) =
    if (storageClass.isNotEmpty() && !DnsSubdomain.matches(storageClass)) "Enter a storage class as it is named in the cluster, or leave it empty." else null

private fun kubernetesJoinSteps(controlPlane: String, name: String, namespace: String, storageClass: String, token: String) = listOf(
    JoinStep(
        "Create the outpost in the cluster",
        "The namespace, a service account, a role limited to that namespace, a Secret holding the token, and the node Deployment. The node joins with the token when it starts.",
        listOf("Copy step 2" to "MYCELIUM_JOIN_TOKEN=$token mycelium node outpost $name --namespace $namespace" + (if (storageClass.isEmpty()) "" else " --storage-class $storageClass") + " --control-plane $controlPlane | kubectl apply -f -"),
    ),
    JoinStep(
        "Add agent sign-ins",
        "Every key is optional: claude-credentials.json, claude.json, codex-auth.json, codex-config.toml. Only this outpost's sessions use this Secret.",
        listOf("Copy step 3" to "kubectl -n $namespace create secret generic $name-agent-credentials \\\n  --from-file=claude-credentials.json=\$HOME/.claude/.credentials.json \\\n  --from-file=codex-auth.json=\$HOME/.codex/auth.json"),
    ),
)

fun joinCommands(method: String, controlPlane: String): List<Pair<String, String>> = when (method) {
    "Shell" -> listOf("Install" to "curl -fsSL $controlPlane/install.sh | sh", "Join" to "mycelium node join $controlPlane")
    "Nix" -> listOf("Join" to "nix run github:mycelium-app/mycelium -- node join $controlPlane")
    else -> listOf(
        "Configuration" to "services.mycelium-node = {\n  enable = true;\n  controlPlane = \"$controlPlane\";\n};",
        "Join" to "sudo mycelium node join",
    )
}

private const val NodesList = "nodes"

@Composable
fun NodesSettings(store: Store, platform: Platform, onOpenNode: (String) -> Unit) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    val loadFailures by store.loadFailures.collectAsState()
    val load = { store.load(NodesList) { store.refreshNodes() } }
    var joinOpen by remember { mutableStateOf(false) }
    var updating by remember { mutableStateOf<NodeSummary?>(null) }
    val nodes = state.nodes.values.sortedWith(compareByDescending<NodeSummary> { it.online }.thenBy { it.name })

    LaunchedEffect(Unit) {
        load()
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label("Nodes", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)

            Box {
                MyButton({ joinOpen = true }, text = "Join a node", icon = MyIcon.Plus, variant = ButtonVariant.Outline)

                if (joinOpen) {
                    JoinPopup(store, store.api.baseUrl, platform, state.experiments.kubernetesNodes, onDismiss = { joinOpen = false })
                }
            }
        }

        if (NodesList in loadFailures && state.nodes.isEmpty()) {
            LoadFailedState("nodes", load)
        } else if (nodes.isEmpty()) {
            Label("No nodes. Choose Join a node to connect the first one.", color = colors.textMuted)
        } else {
            Table(
                listOf(Col("Node", 120.dp, weight = 1f), Col("State", 72.dp), Col("Mycelium", 88.dp)) + HarnessAgents.map { Col(agentName(it), 104.dp) } + Col("", 120.dp, align = ColumnAlign.End),
                nodes.size,
                { row, column ->
                    val node = nodes[row]

                    when (column) {
                        0 -> TableCell.Slot(node.name + if (node.kubernetes != null) " Kubernetes" else "", extra = Tokens.Size.IconSm + Tokens.Space.S2 * 2) {
                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                Icon(nodeIcon(node), nodeColour(node), Tokens.Size.IconSm)
                                Label(node.name, maxLines = 1)

                                if (node.kubernetes != null) {
                                    Tag("Kubernetes")
                                }
                            }
                        }

                        1 -> when {
                            node.draining -> TableCell.Slot("Draining", extra = Tokens.Size.IconSm + Tokens.Space.S1) { StatusMark("Draining", Tone.Attention, MyIcon.Clock) }
                            node.online -> TableCell.Text("Online", color = colors.textMuted)
                            else -> TableCell.Slot("Offline", extra = Tokens.Size.IconSm + Tokens.Space.S1) { StatusMark("Offline", Tone.Muted, MyIcon.Failed) }
                        }

                        2 -> versionCell(node.version, node.updateAvailable)

                        3 + HarnessAgents.size -> if (node.online && node.agents.any { it.available && it.updateAvailable != null }) {
                            TableCell.Slot("Update agents") {
                                MyButton({ updating = node }, text = "Update agents", size = ButtonSize.Sm, variant = ButtonVariant.Outline, description = "Update the agents on ${node.name}")
                            }
                        } else {
                            TableCell.Text("")
                        }

                        else -> {
                            val agent = node.agents.firstOrNull { it.agent == HarnessAgents[column - 3] && it.available }

                            if (agent == null) {
                                TableCell.Text("Not installed", color = colors.textMuted)
                            } else {
                                versionCell(agentVersion(agent.version), agent.updateAvailable)
                            }
                        }
                    }
                },
                onRow = { onOpenNode(nodes[it].id) },
                rowDescription = { "Open ${nodes[it].name}" },
            )
        }

        Label("Sign-ins live in Accounts. Select a node for its agents, resources and ports.", style = Tokens.Type.Meta, color = colors.textMuted)
    }

    updating?.let { node -> AgentUpdatesDialog(store, node, onClose = { updating = null }) }
}

@Composable
private fun AgentUpdatesDialog(store: Store, node: NodeSummary, onClose: () -> Unit) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    val updates = remember(node.id) { node.agents.filter { it.available && it.updateAvailable != null } }
    var started by remember(node.id) { mutableStateOf(emptyMap<String, String>()) }
    var refused by remember(node.id) { mutableStateOf(emptyMap<String, String>()) }

    LaunchedEffect(node.id) {
        for (agent in updates) {
            try {
                started = started + (agent.agent to store.api.updateAgent(node.id, agent.agent).updateId)
            } catch (failure: Exception) {
                refused = refused + (agent.agent to (failure.message ?: "The update could not start."))
            }
        }
    }

    Dialog(onDismiss = onClose) {
        Label("Update agents on ${node.name}", Modifier.semantics { heading() }, Tokens.Type.Title)

        for (agent in updates) {
            val updateId = started[agent.agent]
            val finished = updateId?.let { state.finishedAgentUpdates[it] }
            val output = updateId?.let { state.agentUpdateOutput[it] }.orEmpty()
            val lastLine = withoutTerminalCodes(output).lines().lastOrNull { it.isNotBlank() }?.trim()

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    Label("${agentName(agent.agent)} ${agentVersion(agent.version)} to ${agent.updateAvailable}", Modifier.weight(1f))

                    when {
                        refused[agent.agent] != null || finished == false -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
                        finished == true -> StatusMark("Updated", Tone.Done, MyIcon.Check)
                        else -> StatusMark("Updating", Tone.Working, MyIcon.Working)
                    }
                }

                val detail = refused[agent.agent] ?: lastLine

                if (detail != null && finished != true) {
                    Label(detail, style = Tokens.Type.Meta, color = if (finished == false || refused[agent.agent] != null) colors.statusFailed else colors.textMuted)
                }
            }
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            MyButton(onClose, text = "Close", muted = true)
        }
    }
}

private fun versionCell(version: String, update: String?): TableCell {
    if (update == null) {
        return TableCell.Text(version)
    }

    return TableCell.Slot("$version $update", extra = Tokens.Size.IconSm + Tokens.Space.S1 * 2) {
        Row(Modifier.clearAndSetSemantics { contentDescription = "$version, update to $update" }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
            Label(version, maxLines = 1)
            Icon(MyIcon.ArrowRight, MyTheme.colors.textMuted, Tokens.Size.IconSm)
            Label(update, maxLines = 1)
        }
    }
}

@Composable
private fun JoinPopup(store: Store, controlPlane: String, platform: Platform, kubernetes: Boolean, onDismiss: () -> Unit) {
    var method by remember { mutableStateOf(JoinMethods.first()) }
    var outpostName by remember { mutableStateOf("") }
    var outpostNamespace by remember { mutableStateOf("mycelium") }
    var outpostStorageClass by remember { mutableStateOf("") }
    var joinToken by remember(outpostName) { mutableStateOf<String?>(null) }
    val methods = if (kubernetes) JoinMethods + KubernetesMethod else JoinMethods

    Dialog(onDismiss = onDismiss, width = Tokens.Size.Page) {
        Column(Modifier.weight(1f, fill = false).verticalScrollWithBar(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Label(if (method == KubernetesMethod) "Add an outpost" else "Join a node", Modifier.semantics { heading() }, Tokens.Type.Title)
            Segmented(methods, method, { method = it }, groupLabel = "How the machine runs Mycelium")

            if (method == KubernetesMethod) {
                Label("An outpost is a Kubernetes node in one namespace of an existing cluster. It owns only the pods it creates, so several outposts can share a cluster, or a namespace. Run these with the mycelium command from a machine that can reach the cluster.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)

                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    TextInput(outpostName, { outpostName = it }, label = "Name", modifier = Modifier.weight(1f), error = outpostNameError(outpostName), mono = true)
                    TextInput(outpostNamespace, { outpostNamespace = it }, label = "Namespace", modifier = Modifier.weight(1f), error = outpostNamespaceError(outpostNamespace), mono = true)
                    TextInput(outpostStorageClass, { outpostStorageClass = it }, label = "Storage class", modifier = Modifier.weight(1f), error = outpostStorageClassError(outpostStorageClass), mono = true)
                }

                Label("Leave the storage class empty for a workspace that is lost when a pod is removed.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)

                val ready = outpostName.isNotEmpty() && outpostNamespace.isNotEmpty() && listOf(outpostNameError(outpostName), outpostNamespaceError(outpostNamespace), outpostStorageClassError(outpostStorageClass)).all { it == null }
                if (ready) {
                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        StepHeading(1, "Create a join token", filled = joinToken != null)
                        Label("The token works once, for a node called $outpostName, and expires in an hour. It is shown here only until you close this dialog.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)

                        val created = joinToken

                        if (created == null) {
                            MyButton({ store.act { joinToken = store.api.createJoinToken(outpostName).token } }, text = "Create join token", variant = ButtonVariant.Outline, description = "Create join token")
                        } else {
                            CopyableCode(created, "Join token", platform::copyToClipboard, description = "Copy the join token")
                            MyButton({ store.act { joinToken = store.api.createJoinToken(outpostName).token } }, text = "Create a new join token", size = ButtonSize.Sm, muted = true, description = "Create a new join token")
                        }
                    }
                }

                val created = joinToken
                val steps = if (ready && created != null) kubernetesJoinSteps(controlPlane, outpostName, outpostNamespace, outpostStorageClass, created) else emptyList()

                for ((index, step) in steps.withIndex()) {
                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        StepHeading(index + 2, step.title, filled = false)
                        step.note?.let { Label(it, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted) }

                        for ((description, command) in step.commands) {
                            CopyableCode(command, step.title, platform::copyToClipboard, description = description)
                        }
                    }
                }
            } else {
                for ((label, command) in joinCommands(method, controlPlane)) {
                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Label(label, style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
                        CopyableCode(command, label, platform::copyToClipboard)
                    }
                }
            }
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            StatusMark("Waiting for the next node", Tone.Working, MyIcon.Working)
            MyButton(onDismiss, text = "Cancel", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
fun NodeMenuItems(
    node: NodeSummary,
    close: () -> Unit,
    onRename: () -> Unit,
    onIcon: (String) -> Unit,
    onColour: (String) -> Unit,
    onDrain: () -> Unit,
    onAskRevoke: () -> Unit,
) {
            MenuItem("Rename", onClick = { close(); onRename() }, leading = MyIcon.Pencil)
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

@Composable
fun LoginPanel(
    session: LoginSession,
    output: String,
    finished: Boolean?,
    onOpenUrl: (String) -> Unit,
    onCopy: (String) -> Unit,
    onInput: (String) -> Unit,
    onClose: () -> Unit,
    framed: Boolean = true,
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
        Modifier.fillMaxWidth().then(if (framed) Modifier.background(colors.bgSelected).padding(Tokens.Space.S3) else Modifier).semantics { contentDescription = "${agentName(session.agent)} sign-in" },
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
