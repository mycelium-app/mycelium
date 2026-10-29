package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.zIndex
import mycelium.api.QueuedPrompt
import mycelium.state.BadgeGap
import mycelium.state.ThreadImage
import mycelium.state.imageLabel
import mycelium.state.isImage
import mycelium.state.pastedSummary

class QueueEdit(val number: Int, val onSave: (String) -> Unit, val onCancel: () -> Unit)

@Composable
fun QueueEditStrip(edit: QueueEdit, onSave: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = rowHeight()).semantics { liveRegion = LiveRegionMode.Polite },
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(MyIcon.Pencil, MyTheme.colors.textMuted, Tokens.Size.IconSm)
        Label("Editing queued message ${edit.number}", Modifier.weight(1f), Tokens.Type.UiStrong, maxLines = 1)
        MyButton(edit.onCancel, text = "Cancel", size = ButtonSize.Sm, muted = true, tooltip = "Cancel (Escape)")
        MyButton(onSave, text = "Save", size = ButtonSize.Sm, variant = ButtonVariant.Primary, tooltip = "Save (Enter)")
    }
}

@Composable
fun QueuedMessages(
    queue: List<QueuedPrompt>,
    sending: List<String>,
    onSendNow: (String) -> Unit,
    onSendAllNow: () -> Unit,
    onEdit: (QueuedPrompt) -> Unit,
    onMove: (String, Int) -> Unit,
    onRemove: (String) -> Unit,
) {
    val colors = MyTheme.colors
    var expanded by remember { mutableStateOf(true) }
    var dragging by remember { mutableStateOf<String?>(null) }
    var dragOffset by remember { mutableStateOf(0f) }
    val heights = remember { mutableStateMapOf<String, Int>() }
    val shown by rememberUpdatedState(queue)
    val editable = queue.none { it.editing }

    fun dropIndex(from: Int): Int {
        var index = from
        var travelled = dragOffset

        while (travelled > 0 && index < shown.lastIndex && travelled > (heights[shown[index + 1].id] ?: 0) / 2f) {
            travelled -= heights[shown[index + 1].id] ?: 0
            index++
        }

        while (travelled < 0 && index > 0 && -travelled > (heights[shown[index - 1].id] ?: 0) / 2f) {
            travelled += heights[shown[index - 1].id] ?: 0
            index--
        }

        return index
    }

    Column(Modifier.fillMaxWidth().semantics { contentDescription = "Queued messages" }) {
        Row(
            Modifier.fillMaxWidth().padding(start = Tokens.Space.S1, end = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MyButton(
                { expanded = !expanded },
                text = "${queue.size + sending.size} queued ${if (queue.size + sending.size == 1) "message" else "messages"}",
                description = if (expanded) "Collapse queued messages" else "Expand queued messages",
                size = ButtonSize.Sm,
                muted = true,
                trailingIcon = if (expanded) MyIcon.ChevronDown else MyIcon.ChevronRight,
            )
            Box(Modifier.weight(1f))

            if (queue.isNotEmpty()) {
                MyButton(onSendAllNow, text = "Send all now", size = ButtonSize.Sm, muted = true)
                MyButton({ queue.forEach { onRemove(it.id) } }, text = "Clear all", size = ButtonSize.Sm, muted = true)
            }
        }

        if (!expanded) {
            Hairline()

            return@Column
        }

        val from = queue.indexOfFirst { it.id == dragging }
        val target = if (from >= 0) dropIndex(from) else -1

        for ((index, item) in queue.withIndex()) {
            val images = item.attachments.filter { isImage(it.mediaType) }.map { ThreadImage(it.sha256, it.name, it.mediaType) }
            val currentIndex by rememberUpdatedState(index)
            var menuOpen by remember { mutableStateOf(false) }
            val shift = when {
                item.id == dragging -> dragOffset
                from in 0 until index && index <= target -> -(heights[queue[from].id] ?: 0).toFloat()
                from > index && index >= target && target >= 0 -> (heights[queue[from].id] ?: 0).toFloat()
                else -> 0f
            }

            Row(
                Modifier
                    .fillMaxWidth()
                    .zIndex(if (item.id == dragging) 1f else 0f)
                    .graphicsLayer { translationY = shift }
                    .onSizeChanged { heights[item.id] = it.height }
                    .padding(start = Tokens.Space.S2, end = Tokens.Space.S2, top = Tokens.Space.S1_5, bottom = Tokens.Space.S1_5),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
                verticalAlignment = Alignment.Top,
            ) {
                Box(
                    Modifier
                        .size(Tokens.Size.ControlSm)
                        .clearAndSetSemantics {}
                        .pointerInput(item.id) {
                            detectDragGestures(
                                onDragStart = {
                                    dragging = item.id
                                    dragOffset = 0f
                                },
                                onDrag = { change, amount ->
                                    change.consume()
                                    dragOffset += amount.y
                                },
                                onDragEnd = {
                                    val dropped = dropIndex(currentIndex)

                                    dragging = null
                                    dragOffset = 0f

                                    if (dropped != currentIndex) {
                                        onMove(item.id, dropped)
                                    }
                                },
                                onDragCancel = {
                                    dragging = null
                                    dragOffset = 0f
                                },
                            )
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(MyIcon.Grip, colors.textFaint, Tokens.Size.IconSm)
                }

                Box(Modifier.width(Tokens.Size.IconSm).height(Tokens.Size.ControlSm).semantics { contentDescription = if (index == 0) "Next message" else "Queued" }, contentAlignment = Alignment.Center) {
                    Dot(if (index == 0) colors.statusAttention else colors.textFaint, hollow = index != 0)
                }

                Column(Modifier.weight(1f).alpha(if (item.editing) 0.6f else 1f), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                    val line = buildAnnotatedString {
                        append(pastedSummary(item.text).replace('\n', ' ').trim())

                        for (image in images) {
                            if (length > 0) {
                                append(" ")
                            }

                            append(BadgeGap)

                            withStyle(SpanStyle(fontSize = Tokens.Type.Meta.fontSize, color = colors.textMuted, background = colors.bgPanel)) {
                                append(imageLabel(image.name))
                            }

                            append(BadgeGap)
                        }
                    }

                    Box(Modifier.heightIn(min = Tokens.Size.ControlSm), contentAlignment = Alignment.CenterStart) {
                        BasicText(line, Modifier, Tokens.Type.Ui.themed().copy(color = colors.text), maxLines = 2, overflow = TextOverflow.Ellipsis)
                    }

                    if (images.isNotEmpty()) {
                        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                            for (image in images) {
                                ThreadImageView(image, image.name, tileHeight = Tokens.Size.QueueThumbnail)
                            }
                        }
                    }
                }

                if (!item.editing) {
                    if (index > 0) {
                        SplitButton("Send now", { onSendNow(item.id) }, listOf(SplitAlternative("Move to front", { onMove(item.id, 0) })), variant = ButtonVariant.Outline, size = ButtonSize.Sm, description = "Send queued message ${index + 1} now", menuLabel = "Queued message ${index + 1}")
                    } else {
                        MyButton({ onSendNow(item.id) }, text = "Send now", variant = ButtonVariant.Outline, size = ButtonSize.Sm, description = "Send queued message ${index + 1} now")
                    }

                    Box {
                        MyButton({ menuOpen = true }, icon = MyIcon.MoreVertical, description = "Actions for queued message ${index + 1}", size = ButtonSize.Sm, muted = true, selected = menuOpen)

                        if (menuOpen) {
                            MenuPopup(onDismiss = { menuOpen = false }) {
                                if (editable) {
                                    MenuItem("Edit", { menuOpen = false; onEdit(item) }, leading = MyIcon.Pencil)
                                }

                                if (index > 0) {
                                    MenuItem("Move up", { menuOpen = false; onMove(item.id, index - 1) }, leading = MyIcon.ArrowUp)
                                }

                                if (index < queue.lastIndex) {
                                    MenuItem("Move down", { menuOpen = false; onMove(item.id, index + 1) }, leading = MyIcon.ArrowDown)
                                }

                                Hairline(Modifier.padding(vertical = Tokens.Space.S1))
                                MenuItem("Remove", { menuOpen = false; onRemove(item.id) }, leading = MyIcon.Close)
                            }
                        }
                    }
                }
            }

            Hairline()
        }

        for (text in sending) {
            Row(
                Modifier.fillMaxWidth().heightIn(min = rowHeight()).padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(MyIcon.Working, colors.textMuted, Tokens.Size.IconSm, Modifier.semantics { contentDescription = "Sending" })
                Label(text, Modifier.weight(1f), maxLines = 1)
                Label("Sending", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            Hairline()
        }
    }
}
