package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.Platform
import mycelium.api.SpeechProvider
import mycelium.state.ReadAloudOff
import mycelium.state.ReadAloudProvider
import mycelium.state.SpeechToTextDevice
import mycelium.state.SpeechToTextProvider
import mycelium.state.Store
import mycelium.state.WakePhraseHey
import mycelium.state.WakePhraseOk
import mycelium.state.wakePhraseName

private const val YourProvider = "Your provider"
private const val ThisDevice = "This device"
private const val ReadAloudOffName = "Off"
const val NoSpeechProviderStatement = "Add a provider with a speech-to-text model in Providers to talk to Voice on this computer"
const val DeviceRecogniserWarning = "The recogniser on this device may send your audio to Google."
const val WakePhraseUnavailable = "The wake phrase is not available on this device."

private fun providerStatement(provider: SpeechProvider) = "${provider.name}, ${provider.model}"

@Composable
fun PrivacySettings(store: Store, platform: Platform?, onOpenProviders: () -> Unit = { store.openScreen("settings/providers") }) {
    val colors = MyTheme.colors
    val voice by store.voice.state.collectAsState()
    val device = voice.device
    val privacy = voice.privacy
    val choosable = platform?.dictation() != null && platform.audioRecorder() != null
    val wakeAvailable = platform?.hearsWakePhrase == true
    val speechChoice = store.voice.speechToTextChoice()
    val noSpeechProvider = speechChoice == SpeechToTextProvider && privacy.speechToTextProvider == null
    val readsAloudOff = privacy.textToSpeechProvider == null || device.readAloud == ReadAloudOff

    LaunchedEffect(store) {
        store.voice.load()
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
        Label("Privacy", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Voice", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Checkbox(privacy.voiceEnabled, "Use voice", { store.voice.setEnabled(!privacy.voiceEnabled) }, checkedState = "On", uncheckedState = "Off")
                Label("Use voice")
            }
            Label("Shows the microphone beside Usage. Voice only listens while you have activated it, and no audio is stored.", style = Tokens.Type.Meta, color = colors.textMuted)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Wake phrase on this device", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            if (wakeAvailable) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Checkbox(device.wakePhraseEnabled, "Listen for a wake phrase", { store.voice.setWakePhraseEnabled(!device.wakePhraseEnabled) }, checkedState = "On", uncheckedState = "Off")
                    Label("Listen for a wake phrase")
                }

                if (device.wakePhraseEnabled) {
                    Segmented(listOf(wakePhraseName(WakePhraseHey), wakePhraseName(WakePhraseOk)), wakePhraseName(device.wakePhrase), { picked -> store.voice.setWakePhrase(if (picked == wakePhraseName(WakePhraseOk)) WakePhraseOk else WakePhraseHey) }, groupLabel = "Wake phrase")
                }

                if (device.wakePhraseEnabled && voice.waitingForWakePhrase) {
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .background(colors.bgPanel, BlockShape)
                            .border(Tokens.Size.Hairline, colors.border, BlockShape)
                            .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)
                            .semantics { liveRegion = LiveRegionMode.Polite },
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                    ) {
                        Box(Modifier.size(Tokens.Space.S2).background(colors.statusDone, CircleShape).clearAndSetSemantics {})
                        Label("Waiting for \"${wakePhraseName(device.wakePhrase)}\". Say it, then what you want.", Modifier.weight(1f))
                    }
                }

                Label("Detected on the device only while Mycelium is open. Audio never leaves the device.", style = Tokens.Type.Meta, color = colors.textMuted)
            } else {
                Label(WakePhraseUnavailable, style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Speech to text", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            if (choosable) {
                Segmented(listOf(YourProvider, ThisDevice), if (speechChoice == SpeechToTextDevice) ThisDevice else YourProvider, { picked -> store.voice.setSpeechToText(if (picked == ThisDevice) SpeechToTextDevice else SpeechToTextProvider) }, groupLabel = "Speech to text")
            }

            val provider = privacy.speechToTextProvider

            when {
                speechChoice == SpeechToTextDevice -> Label(DeviceRecogniserWarning, style = Tokens.Type.Meta, color = colors.textMuted)
                provider != null -> Label(providerStatement(provider))
                noSpeechProvider -> FailureBlock {
                    Label(NoSpeechProviderStatement, style = Tokens.Type.Meta)
                    Row { MyButton(onOpenProviders, text = "Providers", variant = ButtonVariant.Outline) }
                }
            }
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label("Reading replies aloud", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            Segmented(
                listOf(YourProvider, ReadAloudOffName),
                if (readsAloudOff) ReadAloudOffName else YourProvider,
                { picked -> if (privacy.textToSpeechProvider != null) store.voice.setReadAloud(if (picked == ReadAloudOffName) ReadAloudOff else ReadAloudProvider) },
                groupLabel = "Reading replies aloud",
            )

            val speaker = privacy.textToSpeechProvider

            when {
                speaker == null -> Label("Add a provider with a text-to-speech model in Providers to hear replies.", style = Tokens.Type.Meta, color = colors.textMuted, modifier = Modifier.padding(top = Tokens.Space.S0_5))
                !readsAloudOff -> Label(providerStatement(speaker), style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }
    }
}
