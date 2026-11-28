package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import mycelium.api.Assistant
import mycelium.api.TodoItem
import mycelium.api.VirtualChat
import mycelium.api.VirtualMessage
import mycelium.state.AppState
import mycelium.state.PlanEntry
import mycelium.state.PlanStatus
import mycelium.state.Store
import mycelium.state.ThreadImage
import mycelium.state.isImage
import mycelium.state.parsedMarkdown
import mycelium.state.planEntries
import mycelium.state.virtualChatsOf

val LocalVirtualSpeaker = compositionLocalOf<Assistant?> { null }

val LocalWorkToggle = compositionLocalOf<(String) -> Unit> { {} }

@Composable
internal fun VirtualBubble(speaker: Assistant?, text: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val tint = namedColour(speaker?.colour) ?: colors.accent

    Column(
        modifier
            .widthIn(max = Tokens.Size.Measure)
            .background(tint.copy(alpha = VirtualTintAlpha), BlockShape)
            .border(Tokens.Size.Hairline, tint, BlockShape)
            .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
    ) {
        if (speaker != null) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                AssistantAvatar(speaker, Tokens.Size.IconMd)
                Label(speaker.name, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }

        MarkdownBlocks(parsedMarkdown(text.trim()))
    }
}

private const val VirtualTintAlpha = 0.14f

@Composable
internal fun WorkedLine(item: mycelium.state.ThreadItem.Worked) {
    val toggle = LocalWorkToggle.current
    val words = "Worked, ${item.tools} ${if (item.tools == 1) "tool" else "tools"}"

    MyButton({ toggle(item.key) }, text = words, icon = if (item.expanded) MyIcon.ChevronDown else MyIcon.ChevronRight, size = ButtonSize.Sm, muted = true, stateDescription = if (item.expanded) "Expanded" else "Collapsed")
}

fun todosOf(chat: VirtualChat): List<PlanEntry> = planEntries(JsonObject(mapOf("entries" to (chat.todos as? JsonArray ?: JsonArray(emptyList())))))

private fun statusName(status: PlanStatus): String = when (status) {
    PlanStatus.Done -> "completed"
    PlanStatus.Running -> "in_progress"
    PlanStatus.Pending -> "pending"
}

internal fun LazyListScope.virtualChatItems(state: AppState, assistant: Assistant, route: Route, onRoute: (Route) -> Unit, onNew: () -> Unit) {
    val chats = state.virtualChatsOf(assistant.id)

    if (chats.isEmpty()) {
        return
    }

    items(chats, key = { "virtual-${it.id}" }) { chat ->
        val here = route == Route.VirtualChat(assistant.id, chat.id)

        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = rowHeight())
                .pressable({ onRoute(Route.VirtualChat(assistant.id, chat.id)) }, description = "${chat.title}${if (chat.unread) ", unread" else ""}${if (chat.closed) ", closed" else ""}", selected = here, shape = BlockShape)
                .padding(start = Tokens.Space.S8, end = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Label(chat.title, Modifier.weight(1f), color = if (chat.closed) MyTheme.colors.textMuted else MyTheme.colors.text, maxLines = 1)

            when {
                chat.unread -> UnreadDot()
                chat.closed -> Label("Closed", style = Tokens.Type.Meta, color = MyTheme.colors.textFaint, maxLines = 1)
            }
        }
    }

    item(key = "virtual-new-${assistant.id}") {
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = rowHeight())
                .pressable(onNew, description = "New virtual chat for ${assistant.name}", shape = BlockShape)
                .padding(start = Tokens.Space.S8, end = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(MyIcon.Plus, MyTheme.colors.textMuted, Tokens.Size.IconSm)
            Label("New virtual chat", color = MyTheme.colors.textMuted, maxLines = 1)
        }
    }
}

@Composable
internal fun NewVirtualChatDialog(store: Store, assistant: Assistant, onDismiss: () -> Unit, onCreated: (VirtualChat) -> Unit) {
    var title by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val runner = rememberAction()

    fun create() {
        if (title.isBlank()) {
            error = "Enter a name for the virtual chat"
            return
        }

        runner { onCreated(store.createVirtualChat(assistant.id, title.trim())) }
    }

    Dialog(onDismiss) {
        Label("New virtual chat", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("A topic of its own under ${assistant.name}. ${assistant.name} answers it from the same session.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
        TextInput(title, { title = it; error = null }, label = "Name", error = error, onSubmit = ::create)
        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::create, text = "Open virtual chat", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
fun VirtualChatScreen(store: Store, state: AppState, assistantId: String, chatId: String, onBack: () -> Unit) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val assistant = state.assistants[assistantId]
    val chat = state.virtualChats[chatId]
    val messages = state.virtualMessages[chatId].orEmpty()
    var text by remember(chatId) { mutableStateOf(TextFieldValue("")) }
    val attached = remember(chatId) { AttachmentDrafts() }
    var menuOpen by remember { mutableStateOf(false) }
    val runner = rememberAction()
    val list = rememberLazyListState()
    val source = remember { MutableInteractionSource() }

    LaunchedEffect(chatId) {
        store.act { store.readVirtualChat(chatId) }
    }

    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) {
            list.animateScrollToItem(messages.lastIndex)
        }
    }

    if (assistant == null || chat == null) {
        return
    }

    fun send() {
        val typed = text.text.trim()

        if (typed.isEmpty() && attached.drafts.isEmpty()) {
            return
        }

        val drafts = attached.drafts
        text = TextFieldValue("")
        attached.replace(emptyList())

        runner {
            val host = assistant.conversationId
            val uploaded = if (host != null) drafts.map { store.api.uploadAttachment(host, it.file.name, it.file.mediaType, it.file.bytes) } else emptyList()

            store.sendVirtualMessage(chatId, typed, uploaded)
        }
    }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            MyButton(onBack, text = assistant.name, icon = MyIcon.ChevronLeft, description = "Back to ${assistant.name}", muted = true, size = ButtonSize.Sm)
            Label("/", color = colors.textFaint)
            Label(chat.title, Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong, maxLines = 1)

            Box {
                MyButton({ menuOpen = true }, icon = MyIcon.MoreVertical, description = "More for ${chat.title}", size = ButtonSize.Sm, muted = true, selected = menuOpen)

                if (menuOpen) {
                    MenuPopup(onDismiss = { menuOpen = false }) {
                        MenuItem(if (chat.closed) "Open virtual chat" else "Close virtual chat", onClick = { menuOpen = false; store.act { store.closeVirtualChat(chatId, !chat.closed) } }, leading = if (chat.closed) MyIcon.Chat else MyIcon.Archive)
                    }
                }
            }
        }

        Hairline()

        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = androidx.compose.foundation.layout.PaddingValues(vertical = Tokens.Space.S3), horizontalAlignment = Alignment.CenterHorizontally) {
            items(messages, key = { it.id }) { message ->
                Box(Modifier.widthIn(max = Tokens.Size.Measure).fillMaxWidth().padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S1_5)) {
                    VirtualChatMessage(store, assistant, message) { cancelled ->
                        text = TextFieldValue(cancelled, TextRange(cancelled.length))
                    }
                }
            }
        }

        Column(Modifier.widthIn(max = Tokens.Size.Measure).fillMaxWidth().align(Alignment.CenterHorizontally).padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            val todos = todosOf(chat)

            if (todos.isNotEmpty()) {
                TodoList(todos, "virtual:$chatId", onSave = { entries, _ -> store.act { store.saveVirtualChatTodos(chatId, entries.map { TodoItem(it.content, statusName(it.status), it.description) }) } })
            }

            runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }
            AttachmentChips(AttachmentControls(attached.drafts, attached.problem, attached::add, attached::remove))

            Column(Modifier.fillMaxWidth().background(colors.bgPanel, BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape).padding(Tokens.Space.S2)) {
                MessageField(text, { text = it }, source, onSubmit = ::send, onPaste = { false }, onKey = { false }, placeholder = "Message ${chat.title}")

                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    MyButton({ platform?.pickFiles(attached::add) }, icon = MyIcon.Paperclip, description = "Attach files", size = ButtonSize.Sm, muted = true)
                    Box(Modifier.weight(1f))
                    MyButton(::send, icon = MyIcon.Send, description = "Send", size = ButtonSize.Sm)
                }
            }
        }
    }
}

@Composable
private fun VirtualChatMessage(store: Store, assistant: Assistant, message: VirtualMessage, onCancelled: (String) -> Unit) {
    if (!message.fromPerson) {
        VirtualBubble(assistant, message.text)
        return
    }

    val (pictures, files) = message.attachments.partition { isImage(it.mediaType) }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        UserMessage(message.text, files, pictures.map { ThreadImage(it.sha256, it.name, it.mediaType) }, null, null, null, false, null, "", waiting = message.sending)

        if (message.sending) {
            Row(Modifier.fillMaxWidth().semantics { contentDescription = "Sending" }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                Label("Sending", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
                MyButton({ store.act { store.cancelVirtualMessage(message.chatId, message.id); onCancelled(message.text) } }, text = "Cancel", size = ButtonSize.Sm, muted = true, description = "Cancel sending this message")
            }
        }
    }
}
