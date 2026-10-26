package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
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
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import mycelium.api.Assistant
import mycelium.api.Conversation
import mycelium.api.PutAssistantSecretRequest
import mycelium.state.AppState
import mycelium.state.Store
import mycelium.state.ThreadBuilder
import mycelium.state.ThreadItem

@Composable
fun AssistantScreen(store: Store, state: AppState, id: String, onOpenSettings: () -> Unit, onOpenConversation: (String) -> Unit = {}, onOpenArtefact: (String) -> Unit = {}) {
    val assistant = state.assistants[id]

    LaunchedEffect(id) {
        store.act { store.refreshAssistants() }
    }

    if (assistant == null) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            Label("Loading the assistant", color = MyTheme.colors.textMuted)
        }

        return
    }

    val earlier = state.conversations.values.filter { it.assistantId == id && it.id != assistant.conversationId && it.parentId == null }.sortedByDescending { it.createdAt }
    val header: @Composable () -> Unit = { AssistantHeader(store, state, assistant, earlier, onOpenSettings) }
    val conversationId = assistant.conversationId

    if (conversationId != null) {
        ConversationScreen(store, state, conversationId, onArchived = {}, onOpenConversation = onOpenConversation, onOpenArtefact = onOpenArtefact, panels = false, header = header)
    } else {
        Column(Modifier.fillMaxSize()) {
            header()
            StartSession(store, state, assistant)
        }
    }
}

@Composable
private fun AssistantHeader(store: Store, state: AppState, assistant: Assistant, earlier: List<Conversation>, onOpenSettings: () -> Unit) {
    val groupName = assistant.groupId?.let { state.groups[it]?.name }
    var showingEarlier by remember(assistant.id) { mutableStateOf(false) }

    Bar {
        LocalOpenList.current?.let { openList ->
            MyButton(openList, icon = MyIcon.Menu, description = "Conversations", muted = true)
        }

        AssistantAvatar(assistant.name)
        Label(assistant.name, Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
        groupName?.let { Label(it, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted, maxLines = 1) }
        Box(Modifier.weight(1f))

        if (assistant.conversationId != null) {
            MyButton({ store.clearAssistant(assistant) }, text = "Clear", icon = MyIcon.Close, variant = ButtonVariant.Outline)
        }

        RowMenu(assistant.name) { close ->
            MenuItem("Settings", onClick = { close(); onOpenSettings() }, leading = MyIcon.Sliders)
        }
    }

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
    var failure by remember(name) { mutableStateOf<String?>(null) }
    val attempt = rememberAttempt { failure = it }

    Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label("${assistant.name} needs $name${description?.let { ": $it" }.orEmpty()}", Modifier.weight(1f))

            if (!setting) {
                MyButton({ setting = true }, text = "Set secret", description = "Set secret $name", variant = ButtonVariant.Outline)
            }
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        if (setting) {
            SecretValueForm(name, onSave = { value -> attempt { store.api.putAssistantSecret(assistant.id, name, PutAssistantSecretRequest(value = value)); setting = false } }, onCancel = { setting = false })
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
private fun ColumnScope.StartSession(store: Store, state: AppState, assistant: Assistant) {
    var message by remember(assistant.id) { mutableStateOf(TextFieldValue("")) }
    var problem by remember(assistant.id) { mutableStateOf<String?>(null) }
    var missing by remember(assistant.id) { mutableStateOf<String?>(null) }
    val waiting = state.sending[assistant.id].orEmpty()

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
            placeholder = "Message ${assistant.name}",
            onSubmit = ::send,
            attachments = AttachmentControls(emptyList(), problem, { problem = "Send your first message, then attach files to the next one." }, {}),
            modifier = Modifier.widthIn(max = readingMeasure()),
            error = missing,
        )
    }
}
