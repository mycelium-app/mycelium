package mycelium.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selectableGroup
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import mycelium.api.OutlineDocumentNode
import mycelium.state.OutlineRow
import mycelium.state.outlineRows

private val TreeIndent = 16.dp

private val TwistyWidth = 24.dp

@Composable
fun OutlineTree(nodes: List<OutlineDocumentNode>, expanded: Set<String>, selectedId: String?, onExpand: (String, Boolean) -> Unit, onSelect: (OutlineDocumentNode) -> Unit, modifier: Modifier = Modifier) {
    val rows = outlineRows(nodes, expanded)
    val source = remember { MutableInteractionSource() }
    val focused by source.collectIsFocusedAsState()
    var active by remember { mutableStateOf(selectedId) }
    val activeIndex = rows.indexOfFirst { it.node.id == active }.takeIf { it >= 0 } ?: rows.indexOfFirst { it.node.id == selectedId }.coerceAtLeast(0)

    LaunchedEffect(selectedId) {
        if (selectedId != null) {
            active = selectedId
        }
    }

    fun parentOf(index: Int): Int? = (index - 1 downTo 0).firstOrNull { rows[it].level < rows[index].level }

    fun handle(key: Key): Boolean {
        val row = rows.getOrNull(activeIndex) ?: return false
        val move = { to: Int? -> to?.let { rows.getOrNull(it) }?.let { active = it.node.id }; true }

        return when (key) {
            Key.DirectionDown -> move((activeIndex + 1).takeIf { it < rows.size })
            Key.DirectionUp -> move((activeIndex - 1).takeIf { it >= 0 })
            Key.MoveHome -> move(0)
            Key.MoveEnd -> move(rows.lastIndex)
            Key.DirectionRight -> when (row.expanded) {
                false -> { onExpand(row.node.id, true); true }
                true -> move(activeIndex + 1)
                null -> true
            }
            Key.DirectionLeft -> if (row.expanded == true) { onExpand(row.node.id, false); true } else move(parentOf(activeIndex))
            Key.Enter, Key.NumPadEnter, Key.Spacebar -> { onSelect(row.node); true }
            else -> false
        }
    }

    Column(
        modifier
            .padding(horizontal = Tokens.Space.S1_5, vertical = Tokens.Space.S1)
            .semantics { contentDescription = "Documents"; selectableGroup() }
            .onKeyEvent { event -> event.type == KeyEventType.KeyDown && handle(event.key) }
            .focusable(interactionSource = source),
    ) {
        rows.forEachIndexed { index, row ->
            OutlineTreeRow(row, row.node.id == selectedId, focused && index == activeIndex, { active = row.node.id; onSelect(row.node) }, { onExpand(row.node.id, it) })
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun OutlineTreeRow(row: OutlineRow, selected: Boolean, active: Boolean, onSelect: () -> Unit, onExpand: (Boolean) -> Unit) {
    val colors = MyTheme.colors
    val shape = RoundedCornerShape(Tokens.Radius.Control)
    val activeState = remember(active) { mutableStateOf(active) }
    val requester = remember { BringIntoViewRequester() }

    LaunchedEffect(active, selected) {
        if (active || selected) {
            requester.bringIntoView()
        }
    }

    Row(
        Modifier
            .fillMaxWidth()
            .height(touchMin(Tokens.Size.ControlMd))
            .bringIntoViewRequester(requester)
            .focusRing(activeState, shape, inside = true, always = true)
            .clip(shape)
            .drawBehind {
                if (selected) {
                    drawRect(colors.bgSelected)
                }
            }
            .hoverHighlight()
            .focusProperties { canFocus = false }
            .clickable(onClick = onSelect)
            .semantics {
                this.selected = selected
                row.expanded?.let { stateDescription = if (it) "Expanded" else "Collapsed" }
            }
            .padding(start = TreeIndent * (row.level - 1), end = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.width(TwistyWidth).height(touchMin(Tokens.Size.ControlMd)).then(if (row.expanded != null) Modifier.pointerInput(row.expanded) { detectTapGestures { onExpand(!row.expanded) } } else Modifier),
            contentAlignment = Alignment.Center,
        ) {
            row.expanded?.let { Icon(if (it) MyIcon.ChevronDown else MyIcon.ChevronRight, colors.textMuted, Tokens.Size.IconSm) }
        }

        Icon(MyIcon.File, colors.textMuted, Tokens.Size.IconSm)
        Label(row.node.title, Modifier.weight(1f), maxLines = 1)
    }
}
