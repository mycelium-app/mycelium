package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.clickable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.semantics.Role
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.time.Clock
import mycelium.Platform
import mycelium.state.Store
import mycelium.state.VoicePhase
import mycelium.state.VoiceProblem
import mycelium.state.VoiceState
import mycelium.state.VoiceStore

const val VoiceShortcutLabel = "Ctrl+Shift+Space"
const val VoiceKeyShortcuts = "Control+Shift+Space Meta+Shift+Space"
private const val MeterBars = 9
private val VoiceDockMaxHeight = 320.dp
private val VoiceSaidMaxWidth = 480.dp
private const val PushToTalkMillis = 500L

data class VoiceCardActions(
    val onStopListening: () -> Unit,
    val onCancel: () -> Unit,
    val onStopTurn: () -> Unit,
    val onStopSpeaking: () -> Unit,
    val onTalkAgain: () -> Unit,
    val onTryAgain: () -> Unit,
    val onTypeInstead: () -> Unit,
    val onOpenConversation: () -> Unit,
    val onOpenProviders: () -> Unit,
    val onOpenAndroidSettings: (() -> Unit)?,
    val onClose: () -> Unit,
)

fun voiceProblemMessage(problem: VoiceProblem, android: Boolean): String = when (problem) {
    VoiceProblem.MicrophoneDenied -> if (android) {
        "Mycelium cannot use the microphone. Allow microphone access for Mycelium in Android Settings, Apps, Mycelium, Permissions, then select Try again."
    } else {
        "Mycelium cannot use the microphone. Allow microphone access for Mycelium in your system settings, then select Try again."
    }

    VoiceProblem.NoMicrophone -> "No microphone found. Connect one, then select Try again."
    VoiceProblem.NoSpeechProvider -> "Add a provider with a speech-to-text model in Settings, Providers to dictate on this device."
    is VoiceProblem.Failed -> problem.message
}

@Composable
fun Modifier.voiceShortcut(voice: VoiceStore): Modifier {
    var heldSince by remember { mutableStateOf<Long?>(null) }

    return onPreviewKeyEvent { event ->
        val chord = event.key == Key.Spacebar && event.isShiftPressed && (event.isCtrlPressed || event.isMetaPressed)

        when {
            !chord -> false
            !voice.state.value.privacy.voiceEnabled -> false
            event.type == KeyEventType.KeyDown -> {
                if (heldSince == null) {
                    heldSince = Clock.System.now().toEpochMilliseconds()
                    voice.toggleListening()
                }

                true
            }

            event.type == KeyEventType.KeyUp -> {
                val since = heldSince
                heldSince = null

                if (since != null && Clock.System.now().toEpochMilliseconds() - since >= PushToTalkMillis) {
                    voice.stopListening()
                }

                true
            }

            else -> false
        }
    }
}

private fun voiceCardActions(store: Store, platform: Platform?) = VoiceCardActions(
    onStopListening = store.voice::stopListening,
    onCancel = store.voice::cancelListening,
    onStopTurn = store.voice::stopTurn,
    onStopSpeaking = store.voice::stopSpeaking,
    onTalkAgain = store.voice::startListening,
    onTryAgain = store.voice::startListening,
    onTypeInstead = store.voice::typeInstead,
    onOpenConversation = store.voice::openThread,
    onOpenProviders = { store.voice.close(); store.openScreen("settings/providers/$SpeechAnchor") },
    onOpenAndroidSettings = platform?.takeIf { it.opensMicrophoneSettings }?.let { opener -> { opener.openMicrophoneSettings() } },
    onClose = store.voice::close,
)

@Composable
fun OpenVoiceCard(store: Store, platform: Platform?, modifier: Modifier = Modifier) {
    val voice by store.voice.state.collectAsState()

    if (!voice.open || !voice.privacy.voiceEnabled) {
        return
    }

    VoiceCard(voice, voiceCardActions(store, platform), modifier)
}

@Composable
fun VoiceDock(store: Store, platform: Platform?, onOpenConversation: (String) -> Unit, modifier: Modifier = Modifier) {
    val voice by store.voice.state.collectAsState()

    val actions = voiceCardActions(store, platform)

    VoiceDockContent(voice, actions.copy(onTypeInstead = store.voice::cancelListening), store.voice::say, onOpenConversation, modifier)
}

@Composable
fun VoiceDockContent(state: VoiceState, actions: VoiceCardActions, onSay: (String) -> Unit, onOpenConversation: (String) -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val listening = state.phase == VoicePhase.Listening
    var typed by remember { mutableStateOf("") }
    val turns = state.earlier + listOfNotNull(state.turn.takeIf { it.said.isNotBlank() })

    Column(
        modifier
            .fillMaxWidth()
            .heightIn(max = VoiceDockMaxHeight)
            .background(colors.bgPanel)
            .onPreviewKeyEvent { event ->
                if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) {
                    if (listening) actions.onCancel() else actions.onClose()
                    true
                } else {
                    false
                }
            }
            .padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S3),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(Tokens.Size.Avatar).background(colors.bgSelected, CircleShape).clearAndSetSemantics {}, contentAlignment = Alignment.Center) {
                Label("V", style = Tokens.Type.Meta, color = colors.textMuted)
            }
            Label("Voice", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
            MyButton(actions.onOpenConversation, icon = MyIcon.Move, description = "Open the Voice thread", size = ButtonSize.Sm, muted = true)
        }

        val hasLog = turns.isNotEmpty() || (state.phase != VoicePhase.Idle && state.phase != VoicePhase.Ended)

        Column(
            Modifier.weight(1f, fill = false).then(if (hasLog) Modifier.padding(top = Tokens.Space.S2) else Modifier).verticalScroll(rememberScrollState()).semantics { liveRegion = LiveRegionMode.Polite },
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        ) {
            for (turn in turns) {
                VoiceSaid { Label(turn.said, style = Tokens.Type.Ui) }

                for (did in turn.did) {
                    Row(
                        Modifier.heightIn(min = Tokens.Space.S6).clickable(role = Role.Button, onClickLabel = "Open that conversation") { onOpenConversation(did.conversationId) },
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(MyIcon.Check, colors.statusDone, Tokens.Size.IconSm)
                        Label(did.text, style = Tokens.Type.Meta, color = colors.textMuted, decoration = LabelDecoration.DottedUnderline)
                    }
                }

                if (turn.reply.isNotBlank()) {
                    Label(turn.reply, style = Tokens.Type.Ui)
                }
            }

            when (val phase = state.phase) {
                VoicePhase.Listening -> VoiceSaid {
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                        LevelMeter(state.level)
                        Label(state.heard.ifBlank { "Listening" }, style = Tokens.Type.Ui, color = if (state.heard.isBlank()) colors.textMuted else colors.text)
                    }
                }

                VoicePhase.Thinking -> Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    Label("Thinking", Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted)
                    MyButton(actions.onStopTurn, text = "Stop", icon = MyIcon.Stop, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }

                VoicePhase.Speaking -> Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                    Label("Speaking", Modifier.weight(1f), Tokens.Type.Meta, colors.textMuted)
                    MyButton(actions.onStopSpeaking, text = "Stop speaking", icon = MyIcon.Stop, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }

                is VoicePhase.Problem -> VoiceProblemBlock(phase.problem, actions)
                VoicePhase.Idle, VoicePhase.Ended -> Unit
            }
        }

        Row(Modifier.padding(top = Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            TextInput(
                typed,
                { typed = it },
                "Talk or type to Voice",
                Modifier.weight(1f),
                placeholder = "Talk or type to Voice",
                showLabel = false,
                onSubmit = {
                    onSay(typed)
                    typed = ""
                },
            )

            if (state.privacy.voiceEnabled) {
                MyButton(
                    if (listening) actions.onStopListening else actions.onTalkAgain,
                    icon = MyIcon.Microphone,
                    description = if (listening) "Stop listening ($VoiceShortcutLabel)" else "Start listening ($VoiceShortcutLabel)",
                    selected = listening,
                    muted = !listening,
                )
            }
        }
    }
}

@Composable
private fun VoiceSaid(content: @Composable () -> Unit) {
    val colors = MyTheme.colors

    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
        Box(Modifier.widthIn(max = VoiceSaidMaxWidth).background(colors.bgRaised, BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape).padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1_5)) {
            content()
        }
    }
}

@Composable
fun VoiceCard(state: VoiceState, actions: VoiceCardActions, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val turn = state.turn

    BoxWithConstraints(modifier) {
        Column(
            Modifier
                .fillMaxWidth()
                .heightIn(max = maxHeight / 2)
                .background(colors.bgRaised, BlockShape)
                .border(Tokens.Size.Hairline, colors.border, BlockShape)
                .onPreviewKeyEvent { event ->
                    if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) {
                        if (state.phase == VoicePhase.Listening) actions.onCancel() else actions.onClose()
                        true
                    } else {
                        false
                    }
                }
                .verticalScroll(rememberScrollState()),
        ) {
            Row(Modifier.fillMaxWidth().padding(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                Label("Voice", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
                MyButton(actions.onOpenConversation, icon = MyIcon.Move, description = "Open the Voice thread", size = ButtonSize.Sm, muted = true)
            }

            Hairline()

            Column(Modifier.fillMaxWidth().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                if (turn.said.isNotBlank() && state.phase != VoicePhase.Listening) {
                    Label(turn.said, style = Tokens.Type.Prose)
                }

                when (val phase = state.phase) {
                    VoicePhase.Idle -> {
                        Label("Select Start listening, or press $VoiceShortcutLabel.", style = Tokens.Type.Meta, color = colors.textMuted)
                        MyButton(actions.onTalkAgain, text = "Start listening", icon = MyIcon.Microphone, variant = ButtonVariant.Outline)
                    }

                    VoicePhase.Listening -> {
                        LevelMeter(state.level)
                        Label(
                            state.heard.ifBlank { "Listening" },
                            Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                            Tokens.Type.Prose,
                            if (state.heard.isBlank()) colors.textMuted else colors.text,
                        )
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                            MyButton(actions.onStopListening, text = "Send", icon = MyIcon.Send, variant = ButtonVariant.Primary)
                            MyButton(actions.onCancel, text = "Cancel", muted = true)
                        }
                    }

                    VoicePhase.Thinking -> {
                        Label("Thinking", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
                        turn.tool?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1) }
                        Row { MyButton(actions.onStopTurn, text = "Stop", icon = MyIcon.Stop, variant = ButtonVariant.Outline) }
                    }

                    VoicePhase.Speaking -> {
                        turn.tool?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1) }
                        Label(turn.reply, style = Tokens.Type.Prose)
                        Label("Speaking", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                            MyButton(actions.onStopSpeaking, text = "Stop speaking", icon = MyIcon.Stop, variant = ButtonVariant.Outline)

                            if (turn.conversationId != null) {
                                MyButton(actions.onOpenConversation, text = "Open conversation", muted = true)
                            }
                        }
                    }

                    VoicePhase.Ended -> {
                        Label(turn.reply, style = Tokens.Type.Prose)

                        if (turn.readingFailed) {
                            Label("The reply could not be read aloud. The text is above.", style = Tokens.Type.Meta, color = colors.textMuted)
                        }

                        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                            MyButton(actions.onTalkAgain, text = "Talk again", icon = MyIcon.Microphone, variant = ButtonVariant.Outline)

                            if (turn.conversationId != null) {
                                MyButton(actions.onOpenConversation, text = "Open conversation", muted = true)
                            }
                        }
                    }

                    is VoicePhase.Problem -> VoiceProblemBlock(phase.problem, actions)
                }
            }
        }
    }
}

@Composable
private fun VoiceProblemBlock(problem: VoiceProblem, actions: VoiceCardActions) {
    val android = actions.onOpenAndroidSettings != null

    FailureBlock {
        Label(voiceProblemMessage(problem, android), style = Tokens.Type.Meta)

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            if (problem == VoiceProblem.NoSpeechProvider) {
                MyButton(actions.onOpenProviders, text = "Set up speech to text", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
            } else {
                MyButton(actions.onTryAgain, text = "Try again", icon = MyIcon.Refresh, size = ButtonSize.Sm, variant = ButtonVariant.Outline)
            }

            if (problem == VoiceProblem.MicrophoneDenied && actions.onOpenAndroidSettings != null) {
                MyButton(actions.onOpenAndroidSettings, text = "Open Android settings", size = ButtonSize.Sm, muted = true)
            }

            MyButton(actions.onTypeInstead, text = "Type instead", size = ButtonSize.Sm, muted = true)
        }
    }
}

@Composable
internal fun FailureBlock(content: @Composable ColumnScope.() -> Unit) {
    Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Box(Modifier.width(Tokens.Space.S0_5).fillMaxHeight().background(MyTheme.colors.statusFailed))

        Column(
            Modifier.weight(1f).semantics { liveRegion = LiveRegionMode.Assertive },
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            content = content,
        )
    }
}

@Composable
private fun LevelMeter(level: Float) {
    val colors = MyTheme.colors
    val clamped = level.coerceIn(0f, 1f)

    Row(
        Modifier.height(Tokens.Size.ControlMd).clearAndSetSemantics {},
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        for (bar in 0 until MeterBars) {
            val shape = 1f - kotlin.math.abs(bar - MeterBars / 2) / (MeterBars / 2f + 1f)
            val tall = (Tokens.Size.ControlMd.value * (0.2f + 0.8f * clamped * shape)).coerceAtLeast(Tokens.Space.S1.value)

            Box(Modifier.width(Tokens.Space.S1).height(tall.dp).background(colors.statusWorking, ControlShape))
        }
    }
}
