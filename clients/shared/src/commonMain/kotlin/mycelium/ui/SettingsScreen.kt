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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.backhandler.BackHandler
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import mycelium.state.Store
import mycelium.ClientVersion
import mycelium.api.Me

enum class SettingsSection(val label: String, val icon: MyIcon, val summary: String? = null) {
    Appearance("Appearance", MyIcon.Sun),
    Accounts("Accounts", MyIcon.Person, "Agent logins and usage"),
    Agents("Agents", MyIcon.ClaudeMark),
    Instructions("Instructions", MyIcon.File, "The AGENTS.md every conversation starts with"),
    Memories("Memories", MyIcon.Info),
    Sessions("Sessions", MyIcon.Key, "Clients and nodes signed in to your account"),
    ChatModels("Chat models", MyIcon.Chip),
    Forges("Forges", MyIcon.Forge),
    Indexing("Indexing", MyIcon.Search),
    Account("Account", MyIcon.Settings, "Sign out and version"),
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun SettingsScreen(store: Store, me: Me, controlPlane: String, onSignOut: () -> Unit) {
    var section by remember { mutableStateOf<SettingsSection?>(null) }
    val touch = LocalTouch.current
    val opened = section

    BackHandler(enabled = touch && opened != null) { section = null }

    Column(Modifier.fillMaxSize()) {
        Bar {
            if (touch && opened != null) {
                MyButton({ section = null }, icon = MyIcon.Back, description = "Back to settings", muted = true)
                Label(opened.label, Modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
            } else {
                BarTitle("Settings")
            }
        }

        if (touch) {
            if (opened == null) {
                Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar()) {
                    for (entry in SettingsSection.entries) {
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = rowHeight())
                                .pressable({ section = entry }, role = Role.Button, shape = BlockShape)
                                .padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S2),
                            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(entry.icon, MyTheme.colors.textMuted)

                            Column(Modifier.weight(1f)) {
                                Label(entry.label, maxLines = 1)
                                entry.summary?.let { Label(it, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted) }
                            }

                            Icon(MyIcon.ChevronRight, MyTheme.colors.textFaint, Tokens.Size.IconSm)
                        }

                        Hairline()
                    }
                }
            } else {
                CompositionLocalProvider(LocalOpenList provides null) {
                    SettingsContent(opened, store, me, controlPlane, onSignOut, Modifier.weight(1f))
                }
            }
        } else {
            Row(Modifier.weight(1f).fillMaxWidth()) {
                Column(Modifier.width(Tokens.Size.SettingsNav).fillMaxHeight().padding(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                    for (entry in SettingsSection.entries) {
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .heightIn(min = rowHeight())
                                .pressable({ section = entry }, role = Role.Tab, selected = entry == (section ?: SettingsSection.Appearance), shape = BlockShape)
                                .semantics { selected = entry == (section ?: SettingsSection.Appearance) }
                                .padding(horizontal = Tokens.Space.S2),
                            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(entry.icon, MyTheme.colors.textMuted)
                            Label(entry.label, maxLines = 1)
                        }
                    }
                }

                VerticalHairline()
                SettingsContent(section ?: SettingsSection.Appearance, store, me, controlPlane, onSignOut, Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun SettingsContent(section: SettingsSection, store: Store, me: Me, controlPlane: String, onSignOut: () -> Unit, modifier: Modifier) {
    Column(
        modifier.fillMaxHeight().verticalScrollWithBar().padding(horizontal = Tokens.Space.S6, vertical = Tokens.Space.S4),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6),
    ) {
        when (section) {
            SettingsSection.Appearance -> AppearanceSettings()
            SettingsSection.Accounts -> AccountsSettings(store)
            SettingsSection.Agents -> AgentsSettings(store)
            SettingsSection.Instructions -> InstructionsSettings(store)
            SettingsSection.Memories -> MemoriesSettings(store)
            SettingsSection.Sessions -> SessionsSettings(store, onSignOut)
            SettingsSection.ChatModels -> ModelProvidersSettings(store)
            SettingsSection.Forges -> ForgesSettings(store)
            SettingsSection.Indexing -> IndexingSettings(store)
            SettingsSection.Account -> AccountSettings(store, me, controlPlane, onSignOut)
        }
    }
}

@Composable
private fun AppearanceSettings() {
    val colors = MyTheme.colors
    val appearance = LocalAppearance.current

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
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

    }
}

@Composable
private fun AccountSettings(store: Store, me: Me, controlPlane: String, onSignOut: () -> Unit) {
    val colors = MyTheme.colors
    var serverVersion by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        serverVersion = runCatching { store.api.version() }.getOrNull()
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Account", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                Label(me.name.ifBlank { me.email }, color = colors.text)
                Label(me.email, style = Tokens.Type.Meta, color = colors.textMuted)
                Label(listOfNotNull("Control plane $controlPlane", serverVersion?.let { "version $it" }).joinToString(" · "), style = Tokens.Type.Meta, color = colors.textMuted)
            }
        val display = appearance.display

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            DisplayChoice("Text size", TextSize.entries, display.textSize, { it.label }) { appearance.onDisplay(display.copy(textSize = it)) }
            DisplayChoice("Density", RowDensity.entries, display.density, { it.label }) { appearance.onDisplay(display.copy(density = it)) }
            DisplayChoice("Chat width", ChatWidth.entries, display.chatWidth, { it.label }) { appearance.onDisplay(display.copy(chatWidth = it)) }
            DisplayChoice("Code size", CodeSize.entries, display.codeSize, { it.label }) { appearance.onDisplay(display.copy(codeSize = it)) }
            DisplayChoice("Reduce motion", MotionSetting.entries, display.reducedMotion, { it.label }) { appearance.onDisplay(display.copy(reducedMotion = it)) }
            DisplayChoice("Times", TimestampStyle.entries, display.timestamps, { it.label }) { appearance.onDisplay(display.copy(timestamps = it)) }
        }
    }
}

@Composable
private fun <T> DisplayChoice(title: String, options: List<T>, chosen: T, label: (T) -> String, onChoose: (T) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label(title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        Segmented(options.map(label), label(chosen), { picked -> onChoose(options.first { label(it) == picked }) }, groupLabel = title)
            MyButton(onSignOut, text = "Sign out", icon = MyIcon.Logout, variant = ButtonVariant.Outline)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("About", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Label("Mycelium $ClientVersion", color = colors.text)
        }
    }
}
