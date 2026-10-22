package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import mycelium.ClientVersion
import mycelium.api.Me

@Composable
fun SettingsScreen(me: Me, controlPlane: String, onSignOut: () -> Unit) {
    val colors = MyTheme.colors
    val appearance = LocalAppearance.current

    Column(Modifier.fillMaxSize()) {
        Bar {
            BarTitle("Settings")
        }

        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(Tokens.Space.S4).widthIn(max = Tokens.Size.Page),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6),
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label("Accent", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    for (accent in appearance.accents) {
                        val chosen = accent == appearance.accent
                        val swatch = if (accent == Accent.System && chosen) colors.accent else if (appearance.dark) accent.dark else accent.light

                        Row(
                            Modifier
                                .border(Tokens.Size.Hairline, if (chosen) colors.text else colors.borderControl, ControlShape)
                                .pressable({ appearance.onAccent(accent) }, role = Role.RadioButton, shape = ControlShape, description = "${accent.label} accent", stateDescription = if (chosen) "Selected" else null)
                                .semantics { selected = chosen }
                                .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5),
                            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Box(Modifier.size(Tokens.Size.IconMd).background(swatch, RoundedCornerShape(Tokens.Radius.Full)))
                            Label(accent.label, maxLines = 1)

                            if (chosen) {
                                Icon(MyIcon.Check, colors.text, Tokens.Size.IconSm)
                            }
                        }
                    }
                }
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Label("Account", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                    Label(me.name.ifBlank { me.email }, color = colors.text)
                    Label(me.email, style = Tokens.Type.Meta, color = colors.textMuted)
                    Label("Control plane $controlPlane", style = Tokens.Type.Meta, color = colors.textMuted)
                }
                MyButton(onSignOut, text = "Sign out", icon = MyIcon.Logout, variant = ButtonVariant.Outline)
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("About", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                Label("Mycelium $ClientVersion", color = colors.text)
            }
        }
    }
}
