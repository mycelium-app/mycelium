package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import mycelium.api.Assistant
import mycelium.api.Conversation
import mycelium.api.PutAssistantSecretRequest
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.VoiceAssistantKey
import mycelium.state.ThreadBuilder
import mycelium.state.ThreadItem

@Composable
fun AssistantScreen(store: Store, state: AppState, id: String, onOpenSettings: () -> Unit, onManageActions: () -> Unit = onOpenSettings, onBack: () -> Unit = {}, onOpenConversation: (String) -> Unit = {}, onOpenArtefact: (String) -> Unit = {}) {
    val assistant = state.assistants[id]

    LaunchedEffect(id) {
        store.load { store.refreshAssistants() }
    }

    if (assistant == null) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Label("Loading the assistant", color = MyTheme.colors.textMuted)
        }

        return
    }

    val earlier = state.conversations.values.filter { it.assistantId == id && it.id != assistant.conversationId && it.parentId == null }.sortedByDescending { it.createdAt }
    val conversationId = assistant.conversationId
    val title: @Composable RowScope.() -> Unit = { AssistantTitle(state, assistant, onBack) }
    val menu: @Composable (() -> Unit) -> Unit = { close -> AssistantMenuItems(store, state, assistant, close, onOpenSettings) }
    val notices: @Composable () -> Unit = { AssistantNotices(store, state, assistant, earlier) }

    val isVoice = assistant.builtinKey == VoiceAssistantKey

    if (conversationId != null && isVoice) {
        CompositionLocalProvider(LocalComposerPlaceholder provides "Ask ${assistant.name}", LocalReadAloud provides store.voice::readAloud) {
            ConversationScreen(store, state, conversationId, onArchived = {}, onOpenConversation = onOpenConversation, onOpenArtefact = onOpenArtefact, panels = false, header = HeaderSlots(title, menu, notices), onManageActions = onManageActions)
        }
    } else if (conversationId != null) {
        ConversationScreen(store, state, conversationId, onArchived = {}, onOpenConversation = onOpenConversation, onOpenArtefact = onOpenArtefact, panels = false, header = HeaderSlots(title, menu, notices), onManageActions = onManageActions)
    } else {
        Column(Modifier.fillMaxSize()) {
            Bar {
                OpenListButton()

                title()
                Box(Modifier.weight(1f))

                val menuState = rememberRowMenuState()

                RowMenu(assistant.name, menuState) { close -> menu(close) }
            }

            notices()
            StartSession(store, state, assistant, onManageActions)
        }
    }
}

@Composable
private fun RowScope.AssistantTitle(state: AppState, assistant: Assistant, onBack: () -> Unit) {
    val groupName = assistant.groupId?.let { state.groups[it]?.name }

    if (LocalOpenList.current != null) {
        BackButton("Assistants", onBack)
    }

    AssistantAvatar(assistant.name, voice = assistant.builtinKey == VoiceAssistantKey)
    Label(assistant.name, Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
    groupName?.let { Label(it, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted, maxLines = 1) }
}

@Composable
private fun AssistantNotices(store: Store, state: AppState, assistant: Assistant, earlier: List<Conversation>) {
    var showingEarlier by remember(assistant.id) { mutableStateOf(false) }

    for (secret in assistant.secrets.filter { it.state == "waiting" }) {
        WaitingSecret(store, assistant, secret.name, secret.description)
    }

    if (earlier.isNotEmpty()) {
        val label = if (earlier.size == 1) "1 earlier session" else "${earlier.size} earlier sessions"

        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = rowHeight())
                .pressable({ showingEarlier = !showingEarlier }, description = "$label, ${if (showingEarlier) "expanded" else "collapsed"}", shape = BlockShape)
                .padding(horizontal = Tokens.Space.S4),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Label(label, Modifier.weight(1f), Tokens.Type.Meta, MyTheme.colors.textMuted)
            Icon(if (showingEarlier) MyIcon.ChevronDown else MyIcon.ChevronRight, MyTheme.colors.textMuted, Tokens.Size.IconSm)
        }

        if (showingEarlier) {
            Column(Modifier.fillMaxWidth().heightIn(max = Tokens.Size.EarlierSessions).verticalScrollWithBar().padding(horizontal = Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                for (session in earlier) {
                    EarlierSession(store, state, session)
                }
            }
        }

        Hairline()
    }
}

@Composable
private fun WaitingSecret(store: Store, assistant: Assistant, name: String, description: String?) {
    var setting by remember(name) { mutableStateOf(false) }
    val runner = rememberAction(name)

    Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label("${assistant.name} needs $name${description?.let { ": $it" }.orEmpty()}", Modifier.weight(1f))

            if (!setting) {
                MyButton({ setting = true }, text = "Set secret", description = "Set secret $name", variant = ButtonVariant.Outline)
            }
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        if (setting) {
            SecretValueForm(name, onSave = { value -> runner { store.api.putAssistantSecret(assistant.id, name, PutAssistantSecretRequest(value = value)); setting = false } }, onCancel = { setting = false })
        }
    }

    Hairline()
}

@Composable
internal fun SecretValueForm(name: String, onSave: (String) -> Unit, onCancel: () -> Unit) {
    var value by remember(name) { mutableStateOf("") }
    var missing by remember(name) { mutableStateOf<String?>(null) }

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
        TextInput(value, { value = it; missing = null }, label = "Value of $name", masked = true, error = missing, modifier = Modifier.widthIn(max = Tokens.Size.FormColumn))
        MyButton({ if (value.isEmpty()) missing = "Enter the value of $name." else onSave(value) }, text = "Save secret", description = "Save secret $name")
        MyButton(onCancel, text = "Cancel", muted = true)
    }
}

@Composable
private fun EarlierSession(store: Store, state: AppState, session: Conversation) {
    val builder = remember(session.id) { ThreadBuilder() }
    val thread = remember(state.events[session.id]) { builder.build(state.events[session.id].orEmpty()) }

    LaunchedEffect(session.id) {
        store.act { store.open(session.id) }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Label("${session.title}, ${timestamp(session.createdAt, true)}", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted, maxLines = 1)
        ThreadView(items = thread, onAnswer = { _, _ -> }, onStopWorkflow = {}, modifier = Modifier.height(Tokens.Size.EarlierThread), agent = session.agent)
    }
}

@Composable
private fun ColumnScope.StartSession(store: Store, state: AppState, assistant: Assistant, onManageActions: () -> Unit) {
    var message by remember(assistant.id) { mutableStateOf(TextFieldValue("")) }
    var problem by remember(assistant.id) { mutableStateOf<String?>(null) }
    var missing by remember(assistant.id) { mutableStateOf<String?>(null) }
    val waiting = state.sending[assistant.id].orEmpty()
    val actions = rememberAssistantActionControls(assistant, state.assistantActions[assistant.id].orEmpty(), onManageActions) { action, inputs ->
        store.act {
            store.api.runAssistantAction(assistant.id, action.id, inputs)
            store.refreshAssistants()
        }
    }

    LaunchedEffect(assistant.id) {
        store.act { store.refreshAssistantActions(assistant.id) }
    }

    fun send() {
        val text = message.text.trim()

        if (text.isEmpty()) {
            missing = "Enter a message for ${assistant.name}."
            return
        }

        missing = null
        store.promptAssistant(assistant, text)
        message = TextFieldValue("", TextRange.Zero)
    }

    ThreadView(items = waiting.mapIndexed { index, text -> ThreadItem.UserMessage("sending-$index", text) }, onAnswer = { _, _ -> }, onStopWorkflow = {}, modifier = Modifier.weight(1f))

    Box(Modifier.fillMaxWidth().padding(Tokens.Space.S4), contentAlignment = Alignment.Center) {
        ComposerCard(
            value = message,
            onValueChange = { message = it; missing = null },
            placeholder = if (assistant.builtinKey == VoiceAssistantKey) "Ask ${assistant.name}" else "Message ${assistant.name}",
            onSubmit = ::send,
            attachments = AttachmentControls(emptyList(), problem, { problem = "Send your first message, then attach files to the next one." }, {}),
            kind = ConversationKind.Assistant,
            modifier = Modifier.widthIn(max = readingMeasure()),
            error = missing,
            assistantActions = actions,
        )
    }
}
