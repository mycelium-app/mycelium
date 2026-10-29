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
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
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

private const val RecentProjects = 3

private val PlaceMenuWidth = 340.dp

fun startableNodes(state: AppState) = state.nodes.values.filter { it.online && !it.draining }.sortedBy { it.name }

@Composable
fun NewConversationScreen(store: Store, state: AppState, onCreated: (Conversation) -> Unit, projectId: String? = null, onOpenNodes: () -> Unit = {}) {
    val colors = MyTheme.colors
    val nodes = startableNodes(state)
    var nodeId by remember { mutableStateOf<String?>(null) }
    var agent by remember { mutableStateOf<String?>(null) }
    var cwd by remember { mutableStateOf("") }
    var prompt by remember { mutableStateOf(TextFieldValue("")) }
    val attached = rememberAttachmentDrafts()
    var cwdError by remember { mutableStateOf<String?>(null) }
    var chosenProject by remember { mutableStateOf(projectId) }
    var withoutProject by remember { mutableStateOf(false) }
    var projectMenuOpen by remember { mutableStateOf(false) }
    var placeMenuOpen by remember { mutableStateOf(false) }
    var settingUpProject by remember { mutableStateOf(false) }
    var browsing by remember { mutableStateOf(false) }
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
    val recent = state.conversations.values.filter { it.projectId != null }.sortedByDescending { it.updatedAt }.mapNotNull { conversation -> projects.firstOrNull { it.id == conversation.projectId } }.distinct().take(RecentProjects)
    val project = if (withoutProject) null else state.projects[chosenProject] ?: recent.firstOrNull()
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
            if (LocalOpenList.current != null) {
                OpenListButton()
            }

            BarTitle("New conversation")
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
                        error = cwdError ?: branchError,
                        project = {
                            Box {
                                MyButton(
                                    { projectMenuOpen = true },
                                    text = project?.name ?: "No project",
                                    icon = MyIcon.Folder,
                                    description = "Project: ${project?.name ?: "No project"}",
                                    muted = true,
                                    trailingIcon = MyIcon.ChevronDown,
                                )

                                if (projectMenuOpen) {
                                    ProjectMenu(
                                        projects,
                                        state.nodes,
                                        project?.id,
                                        "Project",
                                        onDismiss = { projectMenuOpen = false },
                                        recent = recent,
                                        noProjectLabel = "No project, choose a folder",
                                        onNewProject = { projectMenuOpen = false; settingUpProject = true },
                                    ) {
                                        projectMenuOpen = false
                                        withoutProject = it == null
                                        chosenProject = it
                                        worktree = false
                                        cwdError = null
                                        branchError = null
                                    }
                                }
                            }
                        },
                        place = node?.let { placed ->
                            {
                                val where = when {
                                    project == null && cwd.isBlank() -> "Choose a folder"
                                    project == null -> cwd.trim().trimEnd('/').substringAfterLast('/').ifEmpty { cwd.trim() }
                                    worktree -> branch.trim().ifEmpty { "New worktree" }
                                    else -> "Checkout"
                                }
                                val summary = when {
                                    project == null && cwd.isBlank() -> "choose a folder"
                                    project == null -> "folder $where"
                                    worktree -> "new worktree"
                                    else -> "existing checkout"
                                }

                                Box {
                                    MyButton(
                                        { placeMenuOpen = true },
                                        text = "${placed.name} · $where",
                                        icon = nodeIcon(placed),
                                        description = "Place: ${placed.name}, $summary",
                                        muted = true,
                                        trailingIcon = MyIcon.ChevronDown,
                                    )

                                    if (placeMenuOpen) {
                                        MenuPopup(onDismiss = { placeMenuOpen = false }, width = PlaceMenuWidth) {
                                            MenuLabel("Run on")

                                            for (option in nodes) {
                                                MenuItem(
                                                    option.name,
                                                    onClick = {
                                                        if (project != null && option.id != project.nodeId) {
                                                            withoutProject = true
                                                            worktree = false
                                                        }

                                                        nodeId = option.id
                                                    },
                                                    checked = option.id == placed.id,
                                                    hint = if (project == null) null else if (option.id == project.nodeId) project.path else "No checkout, choose a folder",
                                                    leading = nodeIcon(option),
                                                )
                                            }

                                            Hairline(Modifier.padding(vertical = Tokens.Space.S1))

                                            if (project != null) {
                                                MenuLabel("Work in")
                                                MenuItem("Existing checkout", onClick = { worktree = false }, checked = !worktree, leading = MyIcon.Folder)
                                                MenuItem("New worktree", onClick = { worktree = true }, checked = worktree, leading = MyIcon.Branch)

                                                if (worktree) {
                                                    TextInput(
                                                        branch,
                                                        { branch = it },
                                                        label = "New branch",
                                                        modifier = Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
                                                        placeholder = "feature/restore-copy",
                                                        mono = true,
                                                        onSubmit = { placeMenuOpen = false; create() },
                                                        hint = worktreeHint(project, branch, state.conversations.values),
                                                    )
                                                }
                                            } else {
                                                Row(
                                                    Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
                                                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                                                    verticalAlignment = Alignment.Bottom,
                                                ) {
                                                    TextInput(cwd, { cwd = it }, label = "Folder", modifier = Modifier.weight(1f), placeholder = "/home/you/project", mono = true, onSubmit = { placeMenuOpen = false; create() })
                                                    MyButton({ placeMenuOpen = false; browsing = true }, text = "Browse", icon = MyIcon.Folder, description = "Browse folders on ${placed.name}", variant = ButtonVariant.Outline)
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                        notice = if (node == null) ({
                            Row(
                                Modifier.fillMaxWidth().padding(bottom = Tokens.Space.S2),
                                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(MyIcon.Alert, colors.statusAttention, Tokens.Size.IconSm)

                                Column(Modifier.weight(1f).semantics { liveRegion = LiveRegionMode.Polite }) {
                                    NoNodeNotice(colors.textMuted)
                                    Label("Your message stays here.", color = colors.textMuted)
                                }

                                MyButton(onOpenNodes, text = "Nodes", description = "Nodes", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                            }
                        }) else null,
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

                    if (node != null && agents.isEmpty()) {
                        NoAgentNotice(node)
                    }
                }
            }
        }
    }

    if (browsing && node != null) {
        FolderBrowserDialog(store, node.id, node.name, cwd, onChoose = { cwd = it; browsing = false }, onDismiss = { browsing = false })
    }

    if (settingUpProject) {
        NewProjectDialog(store, state, onDismiss = { settingUpProject = false }, onCreated = { created ->
            chosenProject = created.id
            withoutProject = false
            settingUpProject = false
        })
    }

    signIn?.let { session -> AccountSignInDialog(store, session, onClose = { signIn = null }) }
}
