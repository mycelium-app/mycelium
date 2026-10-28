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
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.foundation.border
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.api.ApiException
import mycelium.api.Conversation
import mycelium.api.LoginState
import mycelium.api.Routing
import mycelium.api.CreateChatRequest
import mycelium.api.ModelProvider
import mycelium.state.AgentPick
import mycelium.state.Store
import mycelium.state.RoutingPrefix
import mycelium.state.agentPicks
import mycelium.state.anyAccountId
import mycelium.state.newChatAccounts
import mycelium.state.reachableLogins
import mycelium.state.routingAgent
import mycelium.state.routingChoices
import mycelium.state.routingName

@Composable
fun NewChatScreen(store: Store, onCreated: (Conversation) -> Unit, onOpenSettings: () -> Unit = {}, onOpenNodes: () -> Unit = {}) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var providers by remember { mutableStateOf<List<ModelProvider>?>(null) }
    var providerId by remember { mutableStateOf<String?>(null) }
    var model by remember { mutableStateOf<String?>(null) }
    var message by remember { mutableStateOf(TextFieldValue("")) }
    val attached = rememberAttachmentDrafts()
    var messageError by remember { mutableStateOf<String?>(null) }
    var agentPick by remember { mutableStateOf<AgentPick?>(null) }
    var agentProblem by remember { mutableStateOf<String?>(null) }
    val accountFocus = remember { FocusRequester() }
    val picks = remember(state.nodes) { agentPicks(state.nodes.values) }
    val accounts = remember(state.nodes) { newChatAccounts(state.nodes.values) }
    val routing = remember(state.nodes) { routingChoices(reachableLogins(state.nodes.values).map { it.second }, emptyList(), "starts on").filter { it.id != "$RoutingPrefix${Routing.AnyModel}" } }

    LaunchedEffect(Unit) {
        store.act { providers = store.api.modelProviders() }
    }

    val loaded = providers
    val provider = loaded?.firstOrNull { it.id == providerId } ?: loaded?.firstOrNull { it.models.isNotEmpty() }
    val chosenModel = provider?.models?.firstOrNull { it == model } ?: provider?.defaultModel?.takeIf { it in provider.models } ?: provider?.models?.firstOrNull()

    LaunchedEffect(state.nodes, agentPick) {
        val picked = agentPick

        if (agentProblem != null && picked != null && state.nodes.values.any { node -> node.online && node.logins.any { it.agent == picked.agent && it.state == LoginState.SignedIn && (picked.account == null || it.account == picked.account) } }) {
            agentProblem = null
        }
    }

    fun create() {
        val picked = agentPick

        if (picked == null && (provider == null || chosenModel == null)) {
            return
        }

        val text = message.text.trim()

        if (text.isEmpty() && attached.drafts.isEmpty()) {
            messageError = "Enter a message to start the chat."
            return
        }

        messageError = null
        agentProblem = null
        val files = attached.drafts.map { it.file }
        val request = if (picked != null) CreateChatRequest(agent = picked.agent, account = picked.account, nodeId = picked.nodeId) else CreateChatRequest(provider?.id, chosenModel)

        store.act {
            try {
                if (files.isEmpty()) {
                    onCreated(store.api.createChat(request.copy(prompt = text)))
                } else {
                    val created = store.api.createChat(request)
                    store.send(created.id, text, files)
                    onCreated(created)
                }
            } catch (problem: ApiException) {
                if (problem.code != "no_node_online") {
                    throw problem
                }

                agentProblem = problem.message
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            OpenListButton()
            BarTitle("New chat")
        }

        if (loaded == null) {
            return@Column
        }

        if (provider == null && picks.isEmpty()) {
            Column(Modifier.padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                Label("No chat model is set up. Add a provider in Settings, under Chat models, then start a chat.", color = colors.textMuted)

                Row {
                    MyButton(onOpenSettings, text = "Open Settings", icon = MyIcon.Settings, variant = ButtonVariant.Outline)
                }
            }

            return@Column
        }

        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
            Column(
                Modifier.widthIn(max = readingMeasure()).fillMaxWidth().padding(Tokens.Space.S4),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6),
            ) {
                Label("What's on your mind?", style = Tokens.Type.Display)

                agentProblem?.let { problem ->
                    Column(
                        Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.statusFailed, ControlShape).padding(Tokens.Space.S3).semantics { liveRegion = LiveRegionMode.Assertive },
                        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
                    ) {
                        Label("There is a problem", Modifier.semantics { heading() }, Tokens.Type.UiStrong, colors.statusFailed)
                        MyButton({ runCatching { accountFocus.requestFocus() } }, text = problem, variant = ButtonVariant.Ghost, description = "Go to Account: $problem")
                        Row {
                            MyButton(onOpenNodes, text = "Nodes", icon = MyIcon.Server, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                        }
                    }
                }

                ComposerCard(
                    value = message,
                    onValueChange = { message = it; messageError = null },
                    placeholder = "Ask anything",
                    onSubmit = ::create,
                    attachments = attached.controls(),
                    kind = ConversationKind.NewChat,
                    error = messageError,
                    account = {
                        AccountPicker(
                            AccountPickerControls(
                                title = "Start on",
                                label = agentPick?.label,
                                accounts = accounts,
                                routing = routing,
                                current = agentPick?.let { picked -> if (picked.account == null) anyAccountId(picked.agent) else accounts.firstOrNull { it.agent == picked.agent && it.account == picked.account && it.node?.id == picked.nodeId }?.id },
                                providers = loaded.orEmpty(),
                                providerId = provider?.id.takeIf { agentPick == null },
                                model = chosenModel.takeIf { agentPick == null },
                                recent = recentModels(state.conversations.values, loaded.orEmpty()),
                                onFavourite = { picked, favouriteModel, on -> store.setFavouriteModel({ change -> providers = providers?.let(change) }, picked, favouriteModel, on) },
                                onPickModel = { picked, chosenPick -> agentPick = null; providerId = picked.id; model = chosenPick },
                                focus = accountFocus,
                                problem = agentProblem,
                                onPick = { picked, done ->
                                    val row = accounts.firstOrNull { it.id == picked }

                                    agentPick = if (row != null) AgentPick(row.agent, row.account, row.label, "", row.node?.id) else routingAgent(picked)?.let { AgentPick(it, null, routingName(picked.removePrefix(RoutingPrefix)), "") }
                                    agentProblem = null
                                    done(null)
                                },
                            ),
                            Tokens.Size.AccountControlInCard,
                        )
                    },
                )
            }
        }
    }
}
