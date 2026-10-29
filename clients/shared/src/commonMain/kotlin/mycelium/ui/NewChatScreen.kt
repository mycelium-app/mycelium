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
import mycelium.api.CreateChatRequest
import mycelium.api.ModelProvider
import mycelium.state.PickerAccount
import mycelium.state.Store
import mycelium.state.favouriteOptions
import mycelium.state.modelOptions
import mycelium.state.newChatAccounts

@Composable
fun NewChatScreen(store: Store, onCreated: (Conversation) -> Unit, onOpenSettings: () -> Unit = {}, onOpenNodes: () -> Unit = {}) {
    val colors = MyTheme.colors
    val state by store.state.collectAsState()
    var providers by remember { mutableStateOf<List<ModelProvider>?>(null) }
    var pickedKey by remember { mutableStateOf<String?>(null) }
    var anyModel by remember { mutableStateOf(false) }
    var pinned by remember { mutableStateOf<PickerAccount?>(null) }
    var message by remember { mutableStateOf(TextFieldValue("")) }
    val attached = rememberAttachmentDrafts()
    var messageError by remember { mutableStateOf<String?>(null) }
    var agentProblem by remember { mutableStateOf<String?>(null) }
    val accountFocus = remember { FocusRequester() }
    val accounts = remember(state.nodes) { newChatAccounts(state.nodes.values) }
    val favourites = rememberFavouriteModels(store)

    LaunchedEffect(Unit) {
        store.act { providers = store.api.modelProviders() }
    }

    val loaded = providers
    val options = remember(state.nodes, loaded) { modelOptions(state.nodes.values, null, loaded.orEmpty()) }
    val favouriteOptions = favourites.list.favouriteOptions(options, loaded.orEmpty()).filter { it.available }
    val defaultChatModel = loaded?.firstOrNull { it.models.isNotEmpty() }?.let { provider -> provider.defaultModel?.takeIf { it in provider.models } ?: provider.models.first() }
    val chosenOption = when {
        anyModel -> favouriteOptions.firstOrNull()
        else -> options.firstOrNull { it.key == pickedKey } ?: favouriteOptions.firstOrNull() ?: options.firstOrNull { it.providerId != null && it.model == defaultChatModel } ?: options.firstOrNull()
    }
    val pin = pinned?.takeIf { it.agent == chosenOption?.agent }

    LaunchedEffect(state.nodes, chosenOption, pin) {
        val picked = chosenOption?.agent

        if (agentProblem != null && picked != null && state.nodes.values.any { node -> node.online && node.logins.any { it.agent == picked && it.state == LoginState.SignedIn && (pin?.account == null || it.account == pin.account) } }) {
            agentProblem = null
        }
    }

    fun create() {
        val picked = chosenOption

        if (picked == null) {
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
        val request = if (picked.agent != null) CreateChatRequest(agent = picked.agent, account = pin?.account, nodeId = pin?.node?.id, model = picked.model.ifEmpty { null }) else CreateChatRequest(picked.providerId, picked.model)

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

        if (options.isEmpty()) {
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
                    model = {
                        ModelPicker(
                            ModelPickerControls(
                                label = if (anyModel) "Any model" else chosenOption?.name ?: "Choose a model",
                                options = options,
                                favourites = favourites.list,
                                providers = loaded.orEmpty(),
                                chosen = chosenOption?.choice()?.takeIf { !anyModel },
                                anyChosen = anyModel,
                                onFavourite = favourites.toggle,
                                onPickAny = { anyModel = true; agentProblem = null },
                                onPickModel = { picked -> anyModel = false; pickedKey = picked.key; agentProblem = null },
                                accounts = accounts,
                                account = pin?.id,
                                focus = accountFocus,
                                problem = agentProblem,
                                onPickAccount = { picked, done ->
                                    val row = accounts.firstOrNull { it.id == picked }

                                    pinned = row
                                    agentProblem = null

                                    if (row != null && row.agent != chosenOption?.agent) {
                                        anyModel = false
                                        pickedKey = options.firstOrNull { it.agent == row.agent }?.key
                                    }

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
