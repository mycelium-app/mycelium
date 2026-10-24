package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.state.PinnedCard

internal val PinnedRailWidth: Dp = 320.dp

internal val LocalPinnedKeys = compositionLocalOf<Set<String>> { emptySet() }

val LocalAgentOpener = compositionLocalOf<(String) -> (() -> Unit)?> { { null } }

internal data class PinControls(val label: String, val onUnpin: () -> Unit, val collapsed: Boolean = false, val onToggle: (() -> Unit)? = null)

@Composable
internal fun RowScope.PinButtons(pin: PinControls) {
    if (pin.onToggle != null) {
        MyButton(
            pin.onToggle,
            icon = if (pin.collapsed) MyIcon.ChevronRight else MyIcon.ChevronDown,
            description = "${if (pin.collapsed) "Expand" else "Collapse"} ${pin.label}",
            size = ButtonSize.Sm,
            muted = true,
        )
    }

    MyButton(pin.onUnpin, icon = MyIcon.Close, description = "Unpin ${pin.label}", size = ButtonSize.Sm, muted = true)
}

@Composable
internal fun PinnedReference(label: String) {
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
        Icon(MyIcon.Pin, MyTheme.colors.textMuted, Tokens.Size.IconSm)
        Label("Pinned: $label", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted, maxLines = 1)
    }
}

@Composable
internal fun PinnedRail(cards: List<PinnedCard>, onStopWorkflow: () -> Unit, onUnpin: (String) -> Unit, modifier: Modifier = Modifier) {
    Column(
        modifier
            .width(PinnedRailWidth + Tokens.Space.S4)
            .fillMaxHeight()
            .verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.S4, end = Tokens.Space.S4, bottom = Tokens.Space.S4),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
    ) {
        for (card in cards) {
            PinnedCardView(card, onStopWorkflow, { onUnpin(card.key) }, collapsible = false)
        }
    }
}

@Composable
internal fun PinnedStack(cards: List<PinnedCard>, onStopWorkflow: () -> Unit, onUnpin: (String) -> Unit, modifier: Modifier = Modifier) {
    Column(
        modifier
            .verticalScroll(rememberScrollState())
            .padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S2),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        for (card in cards) {
            PinnedCardView(card, onStopWorkflow, { onUnpin(card.key) }, collapsible = true)
        }
    }
}

@Composable
private fun PinnedCardView(card: PinnedCard, onStopWorkflow: () -> Unit, onUnpin: () -> Unit, collapsible: Boolean) {
    var collapsed by remember(card.key) { mutableStateOf(false) }
    val pin = PinControls(card.label, onUnpin, collapsed && collapsible, if (collapsible) ({ collapsed = !collapsed }) else null)

    if (card.workflow != null) {
        WorkflowCard(card.workflow, onStopWorkflow, pin)

        return
    }

    Card {
        CardHead {
            Icon(MyIcon.Pin, MyTheme.colors.textMuted, Tokens.Size.IconSm)
            Label(card.label, Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong, maxLines = 1)
            PinButtons(pin)
        }

        if (!pin.collapsed && card.ui != null) {
            Column(Modifier.padding(Tokens.Space.S3)) {
                GeneratedUi(card.ui)
            }
        }
    }
}
