package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.api.Conversation
import mycelium.api.CreateConversationRequest
import mycelium.api.LoginState
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.SignInPrefix
import mycelium.state.accountChoiceId
import mycelium.state.accountChoices
import mycelium.state.accountsElsewhere
import mycelium.state.agentName
import mycelium.state.switchableAccounts
import mycelium.state.isLaptop

fun startableNodes(state: AppState) = state.nodes.values.filter { it.online && !it.draining }.sortedBy { it.name }

@Composable
fun NewConversationScreen(store: Store, state: AppState, onCreated: (Conversation) -> Unit, projectId: String? = null) {
    val colors = MyTheme.colors
    val nodes = startableNodes(state)
    var nodeId by remember { mutableStateOf<String?>(null) }
    var agent by remember { mutableStateOf<String?>(null) }
    var cwd by remember { mutableStateOf("") }
    var prompt by remember { mutableStateOf("") }
    var cwdError by remember { mutableStateOf<String?>(null) }
    var nodeMenu by remember { mutableStateOf(false) }
    var chosenProject by remember { mutableStateOf(projectId) }
    var projectMenu by remember { mutableStateOf(false) }
    var worktree by remember { mutableStateOf(false) }
    var branch by remember { mutableStateOf("") }
    var branchError by remember { mutableStateOf<String?>(null) }
    var account by remember { mutableStateOf<String?>(null) }
    var signIn by remember { mutableStateOf<LoginSession?>(null) }

    LaunchedEffect(Unit) {
        store.act { store.refreshProjects() }
    }

    val projects = state.projects.values.sortedBy { it.name }
    val project = state.projects[chosenProject]
    val node = if (project != null) state.nodes[project.nodeId] else nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull()
    val agents = node?.agents?.filter { it.available }.orEmpty()
    val chosenAgent = agents.firstOrNull { it.agent == agent }?.agent ?: agents.firstOrNull()?.agent

    val accounts = switchableAccounts(node, chosenAgent.orEmpty())
    val chosenAccount = accounts.firstOrNull { accountChoiceId(it) == account } ?: accounts.firstOrNull { it.state == LoginState.SignedIn }

    fun create() {
        if (node == null || chosenAgent == null) {
            return
        }

        if (project == null && cwd.isBlank()) {
            cwdError = "Enter the folder the agent should work in, for example /home/you/project."
            return
        }

        if (project != null && worktree && branch.isBlank()) {
            branchError = "Enter a name for the new branch."
            return
        }

        cwdError = null
        branchError = null
        store.act {
            val created = store.api.createConversation(
                CreateConversationRequest(
                    node.id,
                    chosenAgent,
                    account = chosenAccount?.account,
                    cwd = if (project == null) cwd.trim() else null,
                    projectId = project?.id,
                    branch = if (project != null && worktree) branch.trim() else null,
                    prompt = prompt.trim().ifEmpty { null },
                ),
            )
            onCreated(created)
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("New conversation")
        }

        if (node == null) {
            Label("No node is online. Join a node from Nodes, or wait for one to reconnect.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)

            return@Column
        }

        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
            Column(
                Modifier.widthIn(max = Tokens.Size.Page).fillMaxWidth().padding(Tokens.Space.S4),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
            ) {
                if (projects.isNotEmpty()) {
                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Label("Project", style = Tokens.Type.MetaStrong, color = colors.textMuted)

                        Box {
                            MyButton(
                                { projectMenu = true },
                                text = project?.name ?: "A folder",
                                icon = if (project != null) state.nodes[project.nodeId]?.let(::nodeIcon) else MyIcon.Folder,
                                description = "Project: ${project?.name ?: "A folder"}",
                                variant = ButtonVariant.Outline,
                                trailingIcon = MyIcon.ChevronDown,
                            )

                            if (projectMenu) {
                                MenuPopup(onDismiss = { projectMenu = false }) {
                                    MenuLabel("Start in")

                                    for (option in projects) {
                                        MenuItem(
                                            option.name,
                                            onClick = { chosenProject = option.id; projectMenu = false },
                                            checked = option.id == project?.id,
                                            leading = state.nodes[option.nodeId]?.let(::nodeIcon),
                                            hint = state.nodes[option.nodeId]?.name,
                                        )
                                    }

                                    MenuItem("A folder", onClick = { chosenProject = null; projectMenu = false }, checked = project == null, leading = MyIcon.Folder)
                                }
                            }
                        }
                    }
                }

                if (project == null) {
                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Label("Node", style = Tokens.Type.MetaStrong, color = colors.textMuted)

                        Box {
                            MyButton(
                                { nodeMenu = true },
                                text = node.name,
                                icon = nodeIcon(node),
                                description = "Node: ${node.name}",
                                variant = ButtonVariant.Outline,
                                trailingIcon = MyIcon.ChevronDown,
                            )

                            if (nodeMenu) {
                                MenuPopup(onDismiss = { nodeMenu = false }) {
                                    MenuLabel("Run on")

                                    for (option in nodes) {
                                        MenuItem(
                                            option.name,
                                            onClick = { nodeId = option.id; nodeMenu = false },
                                            checked = option.id == node.id,
                                            leading = nodeIcon(option),
                                        )
                                    }
                                }
                            }
                        }
                    }
                }

                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label("Agent", style = Tokens.Type.MetaStrong, color = colors.textMuted)

                    if (agents.isEmpty()) {
                        Label("No agent is installed on ${node.name}. Install Claude Code or Codex there, then try again.", color = colors.statusFailed)
                    } else {
                        Segmented(
                            options = agents.map { agentName(it.agent) },
                            selected = agentName(chosenAgent.orEmpty()),
                            onSelect = { picked -> agent = agents.first { agentName(it.agent) == picked }.agent },
                            groupLabel = "Agent",
                        )
                    }
                }

                if (accounts.isNotEmpty()) {
                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Label("Account", style = Tokens.Type.MetaStrong, color = colors.textMuted)

                        OptionMenu(
                            "Account",
                            accountChoices(accounts, node?.let { accountsElsewhere(state.nodes.values, it, chosenAgent.orEmpty()) }.orEmpty(), node?.name.orEmpty()),
                            chosenAccount?.let(::accountChoiceId),
                            { picked ->
                                if (picked.startsWith(SignInPrefix) && node != null && chosenAgent != null) {
                                    store.act {
                                        val started = store.api.startLogin(node.id, chosenAgent, loginMethodFor(chosenAgent), picked.removePrefix(SignInPrefix))
                                        signIn = LoginSession(started.loginId, chosenAgent)
                                    }
                                } else {
                                    account = picked
                                }
                            },
                        )
                    }
                }

                if (project == null) {
                    TextInput(cwd, { cwd = it }, label = "Folder", placeholder = "/home/you/project", mono = true, error = cwdError, onSubmit = ::create)
                }

                if (project != null) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(worktree, "New worktree", { worktree = !worktree }, checkedState = "On", uncheckedState = "Off")
                        Label("New worktree")
                    }

                    if (worktree) {
                        TextInput(branch, { branch = it }, label = "Branch name", placeholder = "feature/restore-copy", mono = true, error = branchError, onSubmit = ::create)
                    }
                }

                TextInput(prompt, { prompt = it }, label = "First message (optional)", placeholder = "Ask, plan or build", onSubmit = ::create)

                Row {
                    MyButton(::create, text = "Start conversation", variant = ButtonVariant.Primary, size = ButtonSize.Lg)
                }
            }
        }
    }

    signIn?.let { session -> AccountSignInDialog(store, session, onClose = { signIn = null }) }
}
