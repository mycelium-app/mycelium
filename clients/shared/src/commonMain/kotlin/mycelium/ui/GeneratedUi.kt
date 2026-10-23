package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.state.MarkdownBlock
import mycelium.state.UiAction
import mycelium.state.UiNode
import mycelium.state.barChartDescription

data class GeneratedUiActions(
    val onSendMessage: (String) -> Unit = {},
    val onSideChat: (String) -> Unit = {},
    val canOpenFiles: Boolean = false,
)

val LocalGeneratedUiActions = staticCompositionLocalOf { GeneratedUiActions() }

@Composable
fun GeneratedUi(node: UiNode) {
    val colors = MyTheme.colors

    when (node) {
        is UiNode.Column -> Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            for (child in node.children) {
                GeneratedUi(child)
            }
        }

        is UiNode.Row -> FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            for (child in node.children) {
                GeneratedUi(child)
            }
        }

        is UiNode.Heading -> Label(node.text, Modifier.semantics { heading() }, Tokens.Type.Title)

        is UiNode.Text -> Label(node.text, style = Tokens.Type.Prose)

        is UiNode.Table -> TableBlock(MarkdownBlock.Table(node.columns, node.rows))

        is UiNode.BarChart -> {
            val largest = node.bars.maxOf { it.value }.coerceAtLeast(0.0)

            Column(
                Modifier.fillMaxWidth().semantics(mergeDescendants = true) { contentDescription = barChartDescription(node) },
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
            ) {
                node.title?.let { Label(it, style = Tokens.Type.UiStrong) }

                for (bar in node.bars) {
                    val share = if (largest > 0) (bar.value / largest).coerceIn(0.0, 1.0).toFloat() else 0f

                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        Label(bar.label, Modifier.width(110.dp), maxLines = 1)

                        Box(Modifier.weight(1f).height(Tokens.Space.S2).background(colors.bgPanel, ControlShape)) {
                            Box(Modifier.fillMaxWidth(share).fillMaxHeight().background(colors.accent, ControlShape))
                        }

                        Label(bar.display, Modifier.widthIn(min = 60.dp), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                    }
                }
            }
        }

        is UiNode.Progress -> Column(
            Modifier.fillMaxWidth().semantics(mergeDescendants = true) {
                contentDescription = "${node.label}: ${node.display}"
                progressBarRangeInfo = ProgressBarRangeInfo(node.share, 0f..1f)
            },
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label(node.label, Modifier.weight(1f), maxLines = 1)
                Label(node.display, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            Box(Modifier.fillMaxWidth().height(Tokens.Space.S2).background(colors.bgPanel, ControlShape)) {
                Box(Modifier.fillMaxWidth(node.share).fillMaxHeight().background(colors.accent, ControlShape))
            }
        }

        is UiNode.Button -> {
            val actions = LocalGeneratedUiActions.current

            when (val action = node.action) {
                is UiAction.SendMessage -> MyButton({ actions.onSendMessage(action.text) }, text = node.label, icon = MyIcon.Send, variant = ButtonVariant.Outline, size = ButtonSize.Sm)

                is UiAction.SideChat -> MyButton({ actions.onSideChat(action.text) }, text = node.label, icon = MyIcon.Chat, variant = ButtonVariant.Outline, size = ButtonSize.Sm)

                is UiAction.OpenFile -> if (actions.canOpenFiles) {
                    var shown by remember { mutableStateOf(false) }

                    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        MyButton({ shown = !shown }, text = node.label, icon = MyIcon.File, variant = ButtonVariant.Outline, size = ButtonSize.Sm)

                        if (shown) {
                            Label(action.path, style = Tokens.Type.Code, color = colors.textMuted)
                        }
                    }
                } else {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                        Icon(MyIcon.File, colors.textMuted, Tokens.Size.IconSm)
                        Label("${node.label}: ${action.path}", style = Tokens.Type.Code, color = colors.textMuted)
                    }
                }
            }
        }
    }
}
