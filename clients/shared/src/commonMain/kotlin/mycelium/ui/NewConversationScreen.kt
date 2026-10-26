package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
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
import androidx.compose.ui.text.input.TextFieldValue
import mycelium.api.Conversation
import mycelium.api.CreateConversationRequest
import mycelium.api.LoginState
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.Store
import mycelium.state.SignInPrefix
import mycelium.state.accountChoiceId
import mycelium.state.accountChoices
import mycelium.state.accountMenuTitle
import mycelium.state.accountsElsewhere
import mycelium.state.agentName
import mycelium.state.switchableAccounts
import mycelium.state.isLaptop

fun startableNodes(state: AppState) = state.nodes.values.filter { it.online && !it.draining }.sortedBy { it.name }

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun NewConversationScreen(store: Store, state: AppState, onCreated: (Conversation) -> Unit, projectId: String? = null) {
    val colors = MyTheme.colors
    val nodes = startableNodes(state)
    var nodeId by remember { mutableStateOf<String?>(null) }
    var agent by remember { mutableStateOf<String?>(null) }
    var cwd by remember { mutableStateOf("") }
    var prompt by remember { mutableStateOf(TextFieldValue("")) }
    val attached = rememberAttachmentDrafts()
    var cwdError by remember { mutableStateOf<String?>(null) }
    var nodeMenu by remember { mutableStateOf(false) }
    var chosenProject by remember { mutableStateOf(projectId) }
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
        val files = attached.drafts.map { it.file }
        val text = prompt.text.trim()

        store.act {
            val created = store.api.createConversation(
                CreateConversationRequest(
                    node.id,
                    chosenAgent,
                    account = chosenAccount?.account,
                    cwd = if (project == null) cwd.trim() else null,
                    projectId = project?.id,
                    branch = if (project != null && worktree) branch.trim() else null,
                    prompt = text.ifEmpty { null }.takeIf { files.isEmpty() },
                ),
            )

            if (files.isNotEmpty()) {
                store.send(created.id, text, files)
            }

            onCreated(created)
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            val breadcrumb: @Composable () -> Unit = {
                if (projects.isNotEmpty()) {
                    ProjectBreadcrumb(project, projects, state.nodes, onSelect = { chosenProject = it })
                }
            }

            if (LocalOpenList.current != null) {
                BarTitle("New conversation")
                breadcrumb()
            } else {
                breadcrumb()

                if (projects.isNotEmpty()) {
                    Label("/", color = colors.textFaint)
                }

                BarTitle("New conversation")
            }
        }

        if (node == null) {
            Label("No node is online. Join a node from Nodes, or wait for one to reconnect.", Modifier.padding(Tokens.Space.S4), color = colors.textMuted)

            return@Column
        }

        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
            Column(
                Modifier.widthIn(max = readingMeasure()).fillMaxWidth().padding(Tokens.Space.S4),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6),
            ) {
                Label("What are we working on?", style = Tokens.Type.Display)

                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    ComposerCard(
                        value = prompt,
                        onValueChange = { prompt = it },
                        placeholder = "Ask, plan or build",
                        onSubmit = ::create,
                        attachments = attached.controls(),
                        trailing = {
                            if (agents.isNotEmpty()) {
                                OptionMenu("Agent", agents.map { Choice(it.agent, agentName(it.agent)) }, chosenAgent, { agent = it })
                            }

                            if (accounts.isNotEmpty()) {
                                OptionMenu(
                                    "Account",
                                    accountChoices(accounts, node?.let { accountsElsewhere(state.nodes.values, it, chosenAgent.orEmpty()) }.orEmpty(), node?.name.orEmpty()),
                                    chosenAccount?.let(::accountChoiceId),
                                    title = accountMenuTitle(chosenAgent.orEmpty()),
                                    icon = MyIcon.Person,
                                    onSelect = { picked ->
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
                        },
                    )

                    if (agents.isEmpty()) {
                        Label("No agent is installed on ${node.name}. Install Claude Code or Codex there, then try again.", color = colors.statusFailed)
                    }

                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1), itemVerticalAlignment = Alignment.CenterVertically) {
                        if (project == null) {
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

                        if (project != null) {
                            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(worktree, "New worktree", { worktree = !worktree }, checkedState = "On", uncheckedState = "Off")
                                Label("New worktree")
                            }
                        }
                    }

                    if (project == null) {
                        FolderField(store, node, cwd, { cwd = it }, label = "Folder", placeholder = "/home/you/project", error = cwdError, onSubmit = ::create)
                    }

                    if (project != null && worktree) {
                        TextInput(branch, { branch = it }, label = "Branch name", placeholder = "feature/restore-copy", mono = true, error = branchError, onSubmit = ::create)
                    }
                }
            }
        }
    }

    signIn?.let { session -> AccountSignInDialog(store, session, onClose = { signIn = null }) }
}
