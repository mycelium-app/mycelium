package mycelium.ui

import mycelium.state.favouriteOptions
import mycelium.state.modelOptions
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
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import mycelium.api.AnyNodeId
import mycelium.api.AnyNodePick
import mycelium.api.Conversation
import mycelium.api.NodeSummary
import mycelium.api.Project
import mycelium.api.CreateConversationRequest
import mycelium.api.LoginState
import mycelium.state.AppState
import mycelium.state.Choice
import mycelium.state.Store
import mycelium.state.SignInPrefix
import mycelium.state.RoutingPrefix
import mycelium.state.accountChoiceId
import mycelium.state.anyAccountId
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

private const val AgentAccountSeparator = "|"

private const val RecentProjects = 3

private val PlaceMenuWidth = 340.dp

fun startableNodes(state: AppState) = state.nodes.values.filter { it.online && !it.draining }.sortedBy { it.name }

@Composable
fun NewConversationScreen(store: Store, state: AppState, onCreated: (Conversation) -> Unit, projectId: String? = null, onOpenNodes: () -> Unit = {}) {
    val colors = MyTheme.colors
    val nodes = startableNodes(state)
    var nodeId by remember { mutableStateOf<String?>(null) }
    var pickedKey by remember { mutableStateOf<String?>(null) }
    var anyModel by remember { mutableStateOf(false) }
    var anyNode by remember { mutableStateOf(false) }
    var setup by remember { mutableStateOf<ProjectSetup?>(null) }
    var setupJob by remember { mutableStateOf<Job?>(null) }
    var sendAfterSetup by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    var anyNodePick by remember { mutableStateOf<AnyNodePick?>(null) }
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
    var pinnedAccount by remember { mutableStateOf<String?>(null) }
    var signIn by remember { mutableStateOf<LoginSession?>(null) }

    LaunchedEffect(Unit) {
        store.load { store.refreshProjects() }
    }

    val groupFilter = LocalGroupFilter.current
    val projects = state.projects.values.filter { inGroup(it.groupId, groupFilter) }.sortedBy { it.name }
    val recent = state.conversations.values.filter { it.projectId != null }.sortedByDescending { it.updatedAt }.mapNotNull { conversation -> projects.firstOrNull { it.id == conversation.projectId } }.distinct().take(RecentProjects)
    val project = if (withoutProject) null else state.projects[chosenProject] ?: recent.firstOrNull()
    LaunchedEffect(placeMenuOpen, anyNode, state.nodes, project?.id) {
        if ((placeMenuOpen || anyNode) && nodes.isNotEmpty()) {
            anyNodePick = null
            anyNodePick = runCatching { store.api.anyNodePick(project?.id) }.getOrNull()
        }
    }

    val previewedNode = anyNodePick?.let { state.nodes[it.nodeId] }
    val node = when {
        setup != null -> setup?.node
        project != null && anyNode -> previewedNode ?: state.nodes[project.nodeId]
        project != null -> state.nodes[project.nodeId]
        anyNode -> previewedNode ?: nodes.firstOrNull()
        else -> nodes.firstOrNull { it.id == nodeId } ?: nodes.firstOrNull()
    }
    val lastNode = if (project != null) state.nodes[project.nodeId] else state.nodes.values.minByOrNull { it.name }
    val agents = node?.agents?.filter { it.available }.orEmpty()
    val favourites = rememberFavouriteModels(store)
    val options = remember(state.nodes, node) { modelOptions(state.nodes.values, node, emptyList()) }
    val favouriteOptions = favourites.list.favouriteOptions(options, emptyList()).filter { it.available }
    val chosenOption = if (anyModel) null else options.firstOrNull { it.key == pickedKey && it.available } ?: favouriteOptions.firstOrNull() ?: options.firstOrNull { it.available && it.agent in agents.map { held -> held.agent } }
    val chosenAgent = if (anyModel) agents.firstOrNull()?.agent else chosenOption?.agent?.takeIf { picked -> agents.any { it.agent == picked } } ?: agents.firstOrNull()?.agent
    val accounts = switchableAccounts(node, chosenAgent.orEmpty())
    val named = accounts.any { it.state == LoginState.SignedIn && it.account != null }
    val routing = when {
        anyModel -> Routing.AnyModel
        pinnedAccount == null && named -> anyAccountId(chosenAgent.orEmpty()).removePrefix(RoutingPrefix)
        else -> null
    }
    val chosenAccount = if (routing != null) null else accounts.firstOrNull { accountChoiceId(it) == pinnedAccount } ?: accounts.firstOrNull { it.state == LoginState.SignedIn }
    val pinned = if (anyModel) null else accounts.firstOrNull { accountChoiceId(it) == pinnedAccount }

    val accountRows = node?.let { held ->
        agents.flatMap { info ->
            val signedIn = accountsOn(held, info.agent).filter { it.state == LoginState.SignedIn }
            val owned = signedIn.map { PickerAccount("${info.agent}$AgentAccountSeparator${accountChoiceId(it)}", info.agent, it.account, held, true, loginUsage(it)) }

            owned.ifEmpty { listOf(PickerAccount("${info.agent}$AgentAccountSeparator$DefaultAccountId", info.agent, null, held, true, UsageUnknown)) } +
                signInAccounts(state.nodes.values, held, info.agent).map { it.copy(id = "${info.agent}$AgentAccountSeparator${it.id}") }
        }
    }.orEmpty()

    val chosenModel = chosenOption?.model.orEmpty()

    fun startSetup(target: Project, onNode: NodeSummary) {
        setupJob?.cancel()
        sendAfterSetup = false
        setup = ProjectSetup.Running(target, onNode)
        setupJob = scope.launch {
            val failure = attempt {
                val checkout = store.api.checkoutProject(target.id, onNode.id)

                store.refreshProjects()
                chosenProject = checkout.id
                withoutProject = false
                setup = null
            }

            if (failure != null) {
                setup = ProjectSetup.Failed(target, onNode, failure)
            }
        }
    }

    fun create() {
        if (setup is ProjectSetup.Running) {
            sendAfterSetup = true
            return
        }

        if (nodes.isEmpty() || node == null || chosenAgent == null || setup is ProjectSetup.Failed) {
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
                    if (anyNode) AnyNodeId else node.id,
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

    LaunchedEffect(setup, sendAfterSetup) {
        if (setup == null && sendAfterSetup) {
            sendAfterSetup = false
            create()
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
                        setup = setup?.let { current ->
                            {
                                SetupStrip(
                                    current,
                                    SetupActions(
                                        onCancel = { setupJob?.cancel(); sendAfterSetup = false; setup = null },
                                        onRetry = { startSetup(current.project, current.node) },
                                        onStartOnAnyNode = { setupJob?.cancel(); sendAfterSetup = false; setup = null; anyNode = true },
                                    ),
                                )
                            }
                        },
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
                                        setupJob?.cancel()
                                        sendAfterSetup = false
                                        setup = null
                                        withoutProject = it == null
                                        chosenProject = it
                                        worktree = false
                                        cwdError = null
                                        branchError = null
                                    }
                                }
                            }
                        },
                        place = if (nodes.isEmpty()) lastNode?.let { last ->
                            {
                                MyButton(
                                    onOpenNodes,
                                    text = "${last.name} · Offline",
                                    icon = MyIcon.Alert,
                                    description = "Place: ${last.name} is offline",
                                    muted = true,
                                    trailingIcon = MyIcon.ChevronDown,
                                )
                            }
                        } else node?.let { placed ->
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
                                        text = "${if (anyNode) "Any node" else placed.name} · $where",
                                        icon = if (anyNode) MyIcon.Server else nodeIcon(placed),
                                        description = "Place: ${if (anyNode) "Any node" else placed.name}, $summary",
                                        muted = true,
                                        trailingIcon = MyIcon.ChevronDown,
                                    )

                                    if (placeMenuOpen) {
                                        MenuPopup(onDismiss = { placeMenuOpen = false }, width = PlaceMenuWidth) {
                                            MenuLabel("Run on")

                                            MenuItem(
                                                "Any node",
                                                onClick = { setupJob?.cancel(); sendAfterSetup = false; setup = null; anyNode = true; placeMenuOpen = false },
                                                checked = anyNode,
                                                hint = anyNodePick?.let { "${it.name} now, most free" },
                                                leading = MyIcon.Server,
                                            )

                                            for (option in nodes) {
                                                MenuItem(
                                                    option.name,
                                                    onClick = {
                                                        val held = project?.let { checkoutOn(state, it, option.id) }

                                                        setupJob?.cancel()
                                                        sendAfterSetup = false
                                                        setup = null
                                                        placeMenuOpen = false

                                                        when {
                                                            project == null -> Unit
                                                            held != null -> {
                                                                chosenProject = held.id
                                                                withoutProject = false
                                                            }
                                                            cloneable(project) -> startSetup(project, option)
                                                            else -> {
                                                                withoutProject = true
                                                                worktree = false
                                                            }
                                                        }

                                                        anyNode = false
                                                        nodeId = option.id
                                                    },
                                                    checked = !anyNode && option.id == placed.id,
                                                    hint = project?.let { held -> checkoutOn(state, held, option.id)?.path ?: if (cloneable(held)) "Not set up yet, clones ${held.name}" else "No checkout, choose a folder" },
                                                    leading = nodeIcon(option),
                                                )
                                            }

                                            Hairline(Modifier.padding(vertical = Tokens.Space.S1))

                                            MenuItem("Load balancing", onClick = { placeMenuOpen = false; store.openScreen("settings/agents") }, leading = MyIcon.Settings)

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
                        notice = if (nodes.isEmpty()) ({
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
                        model = {
                            if (agents.isNotEmpty()) {
                                ModelPicker(
                                    ModelPickerControls(
                                        label = if (anyModel) "Any model" else listOfNotNull(chosenOption?.name ?: agentName(chosenAgent.orEmpty()), pinned?.account?.substringBefore('@')).joinToString(" · "),
                                        options = options,
                                        favourites = favourites.list,
                                        providers = emptyList(),
                                        chosen = chosenOption?.choice(),
                                        anyChosen = anyModel,
                                        onFavourite = favourites.toggle,
                                        onPickAny = { anyModel = true; pinnedAccount = null },
                                        onPickModel = { picked ->
                                            if (picked.agent != chosenAgent) {
                                                pinnedAccount = null
                                            }

                                            anyModel = false
                                            pickedKey = picked.key
                                        },
                                        accounts = accountRows.filter { it.agent == chosenAgent },
                                        account = pinned?.let { "$chosenAgent$AgentAccountSeparator${accountChoiceId(it)}" },
                                        onPickAccount = { picked, done ->
                                            val pickedAccount = picked?.substringAfter(AgentAccountSeparator)

                                            if (pickedAccount != null && pickedAccount.startsWith(SignInPrefix) && node != null && chosenAgent != null) {
                                                store.act {
                                                    val started = store.api.startLogin(node.id, chosenAgent, loginMethodFor(chosenAgent), pickedAccount.removePrefix(SignInPrefix))
                                                    signIn = LoginSession(started.loginId, chosenAgent)
                                                }
                                            } else {
                                                anyModel = false
                                                pinnedAccount = pickedAccount
                                            }

                                            done(null)
                                        },
                                    ),
                                )
                            }
                        },
                    )

                    if (node != null && nodes.isNotEmpty() && agents.isEmpty()) {
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
