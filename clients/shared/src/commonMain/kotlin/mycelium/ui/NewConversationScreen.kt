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
import mycelium.state.RoutingPrefix
import mycelium.state.accountChoiceId
import mycelium.state.anyAccountId
import mycelium.state.routingAgent
import mycelium.state.routingChoices
import mycelium.api.Routing
import mycelium.state.PickerAccount
import mycelium.state.UsageUnknown
import mycelium.state.loginUsage
import mycelium.state.signInAccounts
import mycelium.state.DefaultAccountId
import mycelium.state.accountsOn
import mycelium.state.agentName
import mycelium.state.inGroup
import mycelium.state.switchableAccounts
import mycelium.state.worktreeHint
import mycelium.state.isLaptop
import mycelium.state.modelMenu

private const val AgentAccountSeparator = "|"

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
    var chosenProject by remember { mutableStateOf(projectId) }
    var worktree by remember { mutableStateOf(false) }
    var branch by remember { mutableStateOf("") }
    var branchError by remember { mutableStateOf<String?>(null) }
    var account by remember { mutableStateOf<String?>(null) }
    var model by remember { mutableStateOf("") }
    var signIn by remember { mutableStateOf<LoginSession?>(null) }

    LaunchedEffect(Unit) {
        store.load { store.refreshProjects() }
    }

    val groupFilter = LocalGroupFilter.current
    val projects = state.projects.values.filter { inGroup(it.groupId, groupFilter) }.sortedBy { it.name }
    val project = state.projects[chosenProject]
    val node = if (project != null) state.nodes[project.nodeId] else nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull()
    val agents = node?.agents?.filter { it.available }.orEmpty()
    val chosenAgent = agents.firstOrNull { it.agent == agent }?.agent ?: agents.firstOrNull()?.agent

    val agentSettings = rememberAgentSettings(store)
    val accounts = switchableAccounts(node, chosenAgent.orEmpty())
    val routing = account?.takeIf { it.startsWith(RoutingPrefix) }?.removePrefix(RoutingPrefix)?.takeIf { it == Routing.AnyModel || it == anyAccountId(chosenAgent.orEmpty()).removePrefix(RoutingPrefix) }
    val chosenAccount = if (routing != null) null else accounts.firstOrNull { accountChoiceId(it) == account } ?: accounts.firstOrNull { it.state == LoginState.SignedIn }

    val shownAccount = chosenAccount ?: node?.let { accountsOn(it, chosenAgent.orEmpty()) }.orEmpty().firstOrNull { it.state == LoginState.SignedIn }
    val accountRows = node?.let { held ->
        agents.flatMap { info ->
            val signedIn = accountsOn(held, info.agent).filter { it.state == LoginState.SignedIn }
            val owned = signedIn.map { PickerAccount("${info.agent}$AgentAccountSeparator${accountChoiceId(it)}", info.agent, it.account, held, true, loginUsage(it)) }

            owned.ifEmpty { listOf(PickerAccount("${info.agent}$AgentAccountSeparator$DefaultAccountId", info.agent, null, held, true, UsageUnknown)) } +
                signInAccounts(state.nodes.values, held, info.agent).map { it.copy(id = "${info.agent}$AgentAccountSeparator${it.id}") }
        }
    }.orEmpty()

    val models = if (node != null && chosenAgent != null) modelMenu(node, chosenAgent, chosenAccount?.account) else null
    val chosenModel = model.takeIf { picked -> models?.choices?.any { it.id == picked } == true }.orEmpty()

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
                    routing = routing,
                    cwd = if (project == null) cwd.trim() else null,
                    projectId = project?.id,
                    branch = if (project != null && worktree) branch.trim() else null,
                    prompt = text.ifEmpty { null }.takeIf { files.isEmpty() },
                    model = chosenModel.ifEmpty { null },
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
                OpenListButton()
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
            NoNodeNotice(colors.textMuted, Modifier.padding(Tokens.Space.S4))

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
                        kind = ConversationKind.NewConversation,
                        account = {
                            if (agents.isNotEmpty()) {
                                AccountPicker(
                                    AccountPickerControls(
                                        title = "Start on",
                                        label = when {
                                            routing == Routing.AnyModel -> "Any model"
                                            routing != null -> "${agentName(chosenAgent.orEmpty())}, any account"
                                            else -> "${agentName(chosenAgent.orEmpty())} · ${shownAccount?.account ?: "Default account"}"
                                        },
                                        accounts = accountRows,
                                        routing = routingChoices(node?.logins.orEmpty(), agentSettings.anyModel, "starts on"),
                                        current = routing?.let { "$RoutingPrefix$it" } ?: chosenAgent?.let { "$it$AgentAccountSeparator${shownAccount?.let(::accountChoiceId) ?: DefaultAccountId}" },
                                        onPick = { picked, done ->
                                            val pickedAgent = picked.substringBefore(AgentAccountSeparator)
                                            val pickedAccount = picked.substringAfter(AgentAccountSeparator)

                                            if (picked.startsWith(RoutingPrefix)) {
                                                account = picked
                                                routingAgent(picked)?.let { agent = it }
                                            } else if (pickedAccount.startsWith(SignInPrefix) && node != null) {
                                                store.act {
                                                    val started = store.api.startLogin(node.id, pickedAgent, loginMethodFor(pickedAgent), pickedAccount.removePrefix(SignInPrefix))
                                                    signIn = LoginSession(started.loginId, pickedAgent)
                                                }
                                            } else {
                                                agent = pickedAgent
                                                account = pickedAccount
                                            }

                                            done(null)
                                        },
                                    ),
                                    Tokens.Size.AccountControlInCard,
                                )
                            }
                        },
                        model = if (models != null) ({ OptionMenu("Model", models.choices, chosenModel, { model = it }, title = models.title) }) else null,
                    )

                    if (agents.isEmpty()) {
                        NoAgentNotice(node)
                    }

                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1), itemVerticalAlignment = Alignment.CenterVertically) {
                        if (project == null) {
                            NodeChooser(nodes, node) { nodeId = it }
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
                        TextInput(branch, { branch = it }, label = "New branch", placeholder = "feature/restore-copy", mono = true, error = branchError, onSubmit = ::create, hint = worktreeHint(project, branch, state.conversations.values))
                    }
                }
            }
        }
    }

    signIn?.let { session -> AccountSignInDialog(store, session, onClose = { signIn = null }) }
}
