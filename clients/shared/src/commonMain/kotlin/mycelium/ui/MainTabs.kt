package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.isTertiaryPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics

class MainTab(val key: String, val title: String, val closeLabel: String? = null, val badge: MyIcon? = null, val badgeLabel: String? = null)

@Composable
fun MainTabStrip(tabs: List<MainTab>, selected: String, onSelect: (String) -> Unit, onClose: (String) -> Unit, groupLabel: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Row(modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).semantics { contentDescription = groupLabel }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        for (tab in tabs) {
            val on = tab.key == selected
            val closing = rememberUpdatedState { tab.closeLabel?.let { onClose(tab.key) } }

            Row(Modifier.pointerInput(tab.key) {
                awaitPointerEventScope {
                    while (true) {
                        val event = awaitPointerEvent(PointerEventPass.Initial)

                        if (event.type == PointerEventType.Press && event.buttons.isTertiaryPressed) {
                            closing.value()
                            event.changes.forEach { it.consume() }
                        }
                    }
                }
            }, verticalAlignment = Alignment.CenterVertically) {
                Column(
                    Modifier
                        .heightIn(min = rowHeight())
                        .pressable({ onSelect(tab.key) }, role = Role.Tab, shape = ControlShape, ringInside = true)
                        .semantics { this.selected = on }
                        .padding(horizontal = Tokens.Space.S2),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                        Label(tab.title, style = if (on) Tokens.Type.UiStrong else Tokens.Type.Ui, color = if (on) colors.text else colors.textMuted, maxLines = 1)

                        if (tab.badge != null) {
                            Icon(tab.badge, colors.statusWorking, Tokens.Size.IconSm, Modifier.semantics { contentDescription = tab.badgeLabel.orEmpty() })
                        }
                    }
                    Box(Modifier.width(Tokens.Space.S6).height(Tokens.Size.FocusRing).background(if (on) colors.text else Color.Transparent))
                }

                if (tab.closeLabel != null) {
                    MyButton({ onClose(tab.key) }, icon = MyIcon.Close, description = tab.closeLabel, size = ButtonSize.Sm, muted = true)
                }
            }
        }
    }
}
