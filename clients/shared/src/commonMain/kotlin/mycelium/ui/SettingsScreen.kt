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
import androidx.compose.runtime.collectAsState
import kotlinx.coroutines.CancellationException
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
import mycelium.state.NotifyFinishedPreference
import mycelium.state.NotifySoundPreference
import mycelium.state.Store
import mycelium.ClientVersion
import mycelium.Platform
import mycelium.api.Me

enum class SettingsSection(val label: String, val icon: MyIcon, val summary: String? = null) {
    Appearance("Appearance", MyIcon.Sun),
    Experiments("Experiments", MyIcon.Sliders),
    Accounts("Providers", MyIcon.Person, "Agent harness accounts, usage and chat models"),
    Agents("Agents", MyIcon.ClaudeMark),
    Import("Import", MyIcon.Archive, "Past agent transcripts and Claude chats"),
    Groups("Groups", MyIcon.Layers, "Instructions, folders and projects for each kind of work"),
    Instructions("Instructions", MyIcon.File, "The AGENTS.md every conversation starts with"),
    Memories("Memories", MyIcon.Info),
    Sessions("Sessions", MyIcon.Key, "Clients and nodes signed in to your account"),
    ChatModels("Chat models", MyIcon.Chip),
    Connections("Agent connections", MyIcon.Send, "Permissions for conversations to message each other"),
    ChatTools("Integrations", MyIcon.Sliders, "Services and tools available to conversations"),
    Forges("Forges", MyIcon.Forge),
    Indexing("Indexing", MyIcon.Search),
    Account("Account", MyIcon.Settings, "Sign out and version"),
}

internal val SettingsGroups = listOf(
    "General" to listOf(SettingsSection.Appearance, SettingsSection.Experiments),
    "Agents and models" to listOf(SettingsSection.Accounts, SettingsSection.Agents, SettingsSection.Connections),
    "Knowledge" to listOf(SettingsSection.Groups, SettingsSection.Instructions, SettingsSection.Memories, SettingsSection.Import, SettingsSection.Indexing),
    "Services" to listOf(SettingsSection.ChatTools, SettingsSection.Forges),
    "Your account" to listOf(SettingsSection.Sessions, SettingsSection.Account),
)

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun SettingsScreen(store: Store, me: Me, controlPlane: String, startOn: SettingsSection? = null, onSignOut: () -> Unit) {
    var section by remember { mutableStateOf(if (startOn == SettingsSection.ChatModels) SettingsSection.Accounts else startOn) }
    var openGroup by remember { mutableStateOf<String?>(null) }
    val touch = LocalTouch.current
    val state by store.state.collectAsState()
    val opened = section
    val showGroup: ((String) -> Unit) = { id -> openGroup = id; section = SettingsSection.Groups }
    val waitingProposals = waitingInstructionProposals(state)

    LaunchedEffect(Unit) {
        try {
            store.refreshInstructionProposals()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
        }
    }

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
                    for ((group, entries) in SettingsGroups) {
                        Label(group, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S2).semantics { heading() }, Tokens.Type.MetaStrong, MyTheme.colors.textMuted)

                        for (entry in entries) {
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

                                if (entry == SettingsSection.Instructions && waitingProposals > 0) {
                                    CountBadge(waitingProposals, "$waitingProposals proposed instruction edits waiting")
                                }

                                Icon(MyIcon.ChevronRight, MyTheme.colors.textFaint, Tokens.Size.IconSm)
                            }

                            Hairline()
                        }
                    }
                }
            } else {
                CompositionLocalProvider(LocalOpenList provides null) {
                    SettingsContent(opened, store, me, controlPlane, onSignOut, openGroup, { openGroup = it }, showGroup, Modifier.weight(1f))
                }
            }
        } else {
            Row(Modifier.weight(1f).fillMaxWidth()) {
                Column(Modifier.width(Tokens.Size.SettingsNav).fillMaxHeight().verticalScrollWithBar().padding(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                    for ((group, entries) in SettingsGroups) {
                        Label(group, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S2).semantics { heading() }, Tokens.Type.MetaStrong, MyTheme.colors.textMuted)

                        for (entry in entries) {
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .heightIn(min = rowHeight())
                                    .pressable({ section = entry }, role = Role.Tab, selected = entry == (opened ?: SettingsSection.Appearance), shape = BlockShape)
                                    .semantics { selected = entry == (opened ?: SettingsSection.Appearance) }
                                    .padding(horizontal = Tokens.Space.S2),
                                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(entry.icon, MyTheme.colors.textMuted)
                                Label(entry.label, maxLines = 1)

                                if (entry == SettingsSection.Instructions && waitingProposals > 0) {
                                    CountBadge(waitingProposals, "$waitingProposals proposed instruction edits waiting")
                                }
                            }
                        }
                    }
                }

                VerticalHairline()
                SettingsContent(opened ?: SettingsSection.Appearance, store, me, controlPlane, onSignOut, openGroup, { openGroup = it }, showGroup, Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun SettingsContent(section: SettingsSection, store: Store, me: Me, controlPlane: String, onSignOut: () -> Unit, openGroup: String?, onOpenGroup: (String?) -> Unit, showGroup: ((String) -> Unit)?, modifier: Modifier) {
    Column(
        modifier.fillMaxHeight().verticalScrollWithBar().padding(horizontal = Tokens.Space.S6, vertical = Tokens.Space.S4),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6),
    ) {
        when (section) {
            SettingsSection.Appearance -> AppearanceSettings()
            SettingsSection.Experiments -> ExperimentsSettings(store)
            SettingsSection.Accounts, SettingsSection.ChatModels -> {
                AccountsSettings(store)
                ModelProvidersSettings(store)
            }
            SettingsSection.Agents -> AgentsSettings(store)
            SettingsSection.Import -> ImportSettings(store)
            SettingsSection.Groups -> GroupsSettings(store, openGroup, onOpenGroup)
            SettingsSection.Instructions -> InstructionsSettings(store, showGroup)
            SettingsSection.Memories -> MemoriesSettings(store)
            SettingsSection.Sessions -> SessionsSettings(store, onSignOut)
            SettingsSection.Connections -> ConversationConnectionsSettings(store)
            SettingsSection.ChatTools -> ChatToolsSettings(store)
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

        val display = appearance.display

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            DisplayChoice("Text size", TextSize.entries, display.textSize, { it.label }) { appearance.onDisplay(display.copy(textSize = it)) }
            DisplayChoice("Density", RowDensity.entries, display.density, { it.label }) { appearance.onDisplay(display.copy(density = it)) }
            DisplayChoice("Chat width", ChatWidth.entries, display.chatWidth, { it.label }) { appearance.onDisplay(display.copy(chatWidth = it)) }
            DisplayChoice("Code size", CodeSize.entries, display.codeSize, { it.label }) { appearance.onDisplay(display.copy(codeSize = it)) }
            DisplayChoice("Reduce motion", MotionSetting.entries, display.reducedMotion, { it.label }) { appearance.onDisplay(display.copy(reducedMotion = it)) }
            DisplayChoice("Times", TimestampStyle.entries, display.timestamps, { it.label }) { appearance.onDisplay(display.copy(timestamps = it)) }
        }

        val platform = LocalPlatform.current

        if (platform != null) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Notifications", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
                PreferenceSwitch(platform, NotifyFinishedPreference, "Notify when an agent finishes", onTurnedOn = platform::requestNotificationPermission)
                PreferenceSwitch(platform, NotifySoundPreference, "Play a sound")
            }
        }
    }
}

@Composable
private fun PreferenceSwitch(platform: Platform, key: String, label: String, onTurnedOn: () -> Unit = {}) {
    var on by remember { mutableStateOf(platform.loadPreference(key) != false.toString()) }

    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Checkbox(on, label, {
            on = !on
            platform.savePreference(key, on.toString())

            if (on) {
                onTurnedOn()
            }
        }, checkedState = "On", uncheckedState = "Off")
        Label(label)
    }
}

@Composable
private fun <T> DisplayChoice(title: String, options: List<T>, chosen: T, label: (T) -> String, onChoose: (T) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label(title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        Segmented(options.map(label), label(chosen), { picked -> onChoose(options.first { label(it) == picked }) }, groupLabel = title)
    }
}

@Composable
private fun AccountSettings(store: Store, me: Me, controlPlane: String, onSignOut: () -> Unit) {
    val colors = MyTheme.colors
    var serverVersion by remember { mutableStateOf<String?>(null) }
    var showingChanges by remember { mutableStateOf(false) }

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
            MyButton(onSignOut, text = "Sign out", icon = MyIcon.Logout, variant = ButtonVariant.Outline)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("About", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Label("Mycelium $ClientVersion", color = colors.text)
            MyButton({ showingChanges = true }, text = "What's new", variant = ButtonVariant.Outline)
        }
    }

    if (showingChanges) {
        RecentReleasesDialog(onClose = { showingChanges = false })
    }
}
