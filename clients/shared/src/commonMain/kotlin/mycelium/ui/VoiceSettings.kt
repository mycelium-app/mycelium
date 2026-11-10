package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.api.AssistantPreferences
import mycelium.api.LocalSpeechModel
import mycelium.api.ModelProvider
import mycelium.api.PatchAssistantRequest
import mycelium.api.SpeechProvider
import mycelium.api.SpeechRequest
import mycelium.state.Choice
import mycelium.state.HeardLevel
import mycelium.state.LevelPollMillis
import mycelium.state.PauseMillis
import mycelium.state.ReadAloudOff
import mycelium.state.ReadAloudProvider
import mycelium.state.SpeechSpeedNormal
import mycelium.state.SpeechToTextDevice
import mycelium.state.SpeechToTextProvider
import mycelium.state.Store
import mycelium.state.VoiceAssistantKey
import mycelium.state.VoiceProblem
import mycelium.state.WakePhraseHey
import mycelium.state.WakePhraseOk
import mycelium.state.hasSpeechToText
import mycelium.state.wakePhraseName

private const val OnTheServer = "On the server"
private const val ThisDevice = "This device"
private const val YourProvider = "Your provider"
private const val ReadAloudOffName = "Off"
private const val SpeechToTextUse = "speech_to_text"
private const val OtherModels = "Other models"
private const val DownloadingState = "downloading"
private const val ReadyState = "ready"
private const val FailedState = "failed"
private const val SampleSentence = "This is how replies will sound."
private const val BytesPerMegabyte = 1_000_000L
private const val DownloadPollMillis = 1_000L

const val NoSpeechProviderStatement = "Nothing turns speech into text yet. Choose a provider with a speech-to-text model, such as Voxtral on OpenRouter, or a local model on your server."
const val NoTextToSpeechStatement = "Add a provider with a text-to-speech model to hear replies."
const val DeviceRecogniserWarning = "The recogniser on this device may send your audio to Google."
const val WakePhraseUnavailable = "The wake phrase is not available on this device."
const val EmptyLocalModelStatement = "Enter a Hugging Face model id, such as owner/name."

private val StackedBelow = 480.dp

private val SettingNameWidth = 120.dp

private val FactNameWidth = 180.dp

private val SpeechSpeeds = listOf("slower" to "Slower", SpeechSpeedNormal to "Normal", "faster" to "Faster")

private class SpeechUse(val id: String, val name: String, val recognisedTitle: String, val chosen: SpeechProvider?, val local: Boolean, val recognised: (ModelProvider) -> List<String>)

private fun megabytes(bytes: Long): String = "${(bytes + BytesPerMegabyte / 2) / BytesPerMegabyte} MB"

private fun hostOf(provider: SpeechProvider, providers: List<ModelProvider>): String {
    val host = providers.firstOrNull { it.id == provider.providerId }?.baseUrl?.substringAfter("://")?.substringBefore('/')

    return if (host == null) provider.name else "${provider.name} ($host)"
}

@Composable
fun VoiceSettings(store: Store, platform: Platform?) {
    val colors = MyTheme.colors
    val voice by store.voice.state.collectAsState()
    val app by store.state.collectAsState()
    val device = voice.device
    val privacy = voice.privacy
    val local = privacy.speechToTextLocal
    val provider = privacy.speechToTextProvider
    val speaker = privacy.textToSpeechProvider
    val assistant = app.assistants.values.firstOrNull { it.builtinKey == VoiceAssistantKey }
    val recorder = remember(platform) { platform?.audioRecorder() }
    val choosable = platform?.dictation() != null && recorder != null
    val wakeAvailable = platform?.hearsWakePhrase == true
    val speechChoice = store.voice.speechToTextChoice()
    val readsAloudOff = speaker == null || device.readAloud == ReadAloudOff
    val downloading = local?.state == DownloadingState
    var providers by remember { mutableStateOf<List<ModelProvider>>(emptyList()) }
    var changing by remember { mutableStateOf<String?>(null) }
    var recording by remember { mutableStateOf(false) }
    var heard by remember { mutableStateOf<String?>(null) }
    val openAiCompatible = providers.filter { it.kind == "openai_compatible" }
    val uses = listOf(
        SpeechUse(SpeechToTextUse, "Speech to text", "Speech-to-text models", provider, true) { it.speechToTextModels },
        SpeechUse("text_to_speech", "Reading replies aloud", "Text-to-speech models", speaker, false) { it.textToSpeechModels },
    )
    val dictationRunner = rememberAction()
    val speakingRunner = rememberAction()
    val preferencesRunner = rememberAction()
    val experimentsRunner = rememberAction()

    fun savePreferences(change: (AssistantPreferences) -> AssistantPreferences) {
        val target = assistant ?: return

        preferencesRunner {
            store.patchAssistant(target, PatchAssistantRequest(preferences = change(target.preferences)))
        }
    }

    fun finishRecording() {
        val held = recorder ?: return

        recording = false
        dictationRunner {
            heard = store.api.transcribe(held.stop())
        }
    }

    fun toggleRecording() {
        val held = recorder ?: return

        if (recording) {
            finishRecording()

            return
        }

        heard = null
        dictationRunner.failure = null
        recording = true
        held.start { problem ->
            if (problem != null) {
                recording = false
                dictationRunner.failure = when (problem) {
                    DictationProblem.Denied -> voiceProblemMessage(VoiceProblem.MicrophoneDenied, platform?.opensMicrophoneSettings == true)
                    DictationProblem.NoMicrophone -> voiceProblemMessage(VoiceProblem.NoMicrophone, false)
                    is DictationProblem.Failed -> problem.message
                }
            }
        }
    }

    fun readSampleAloud() {
        val output = platform?.audioPlayer()

        if (output == null) {
            speakingRunner.failure = "This device cannot play audio."

            return
        }

        speakingRunner {
            val audio = store.api.speech(SpeechRequest(SampleSentence, assistant?.preferences?.voiceName, assistant?.preferences?.speechSpeed))

            output.play(audio.bytes, audio.mediaType) {}
        }
    }

    LaunchedEffect(store) {
        store.voice.load()
        store.voice.loadVoices()
        store.load("providers") { providers = store.api.modelProviders() }
        store.load("assistants") { store.loadAssistants() }
    }

    LaunchedEffect(downloading) {
        while (downloading) {
            delay(DownloadPollMillis)
            store.voice.load()
        }
    }

    LaunchedEffect(recording) {
        var spoke = false
        var quietFor = 0L

        while (recording) {
            val level = recorder?.level() ?: 0f

            if (level >= HeardLevel) {
                spoke = true
                quietFor = 0
            } else if (spoke) {
                quietFor += LevelPollMillis
            }

            if (spoke && quietFor >= PauseMillis) {
                finishRecording()
            } else {
                delay(LevelPollMillis)
            }
        }
    }

    BoxWithConstraints(Modifier.widthIn(max = Tokens.Size.Page)) {
        val narrow = maxWidth < StackedBelow

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            if (!LocalTouch.current) {
                Label("Voice", Modifier.semantics { heading() }, Tokens.Type.Title)
            }

            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                    Checkbox(privacy.voiceEnabled, "Use voice", { store.voice.setEnabled(!privacy.voiceEnabled) }, checkedState = "On", uncheckedState = "Off")
                    Label("Use voice")
                }
                Label("Shows the microphone beside Usage. Voice only listens while you have activated it.", Modifier.padding(start = Tokens.Space.S8 + Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)
            }

            VoiceStep("1", "How you talk") {
                if (wakeAvailable) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        Checkbox(device.wakePhraseEnabled, "Listen for a wake phrase", { store.voice.setWakePhraseEnabled(!device.wakePhraseEnabled) }, checkedState = "On", uncheckedState = "Off")
                        Label("Listen for a wake phrase")
                    }

                    if (device.wakePhraseEnabled) {
                        Segmented(listOf(wakePhraseName(WakePhraseHey), wakePhraseName(WakePhraseOk)), wakePhraseName(device.wakePhrase), { picked -> store.voice.setWakePhrase(if (picked == wakePhraseName(WakePhraseOk)) WakePhraseOk else WakePhraseHey) }, groupLabel = "Wake phrase")
                    }

                    if (device.wakePhraseEnabled && voice.waitingForWakePhrase) {
                        StatusBlock {
                            Box(Modifier.size(Tokens.Space.S2).background(colors.statusDone, CircleShape).clearAndSetSemantics {})
                            Label("Waiting for \"${wakePhraseName(device.wakePhrase)}\". Say it, then what you want.", Modifier.weight(1f))
                        }
                    }
                } else {
                    Label(WakePhraseUnavailable, style = Tokens.Type.Meta, color = colors.textMuted)
                }

                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                        Checkbox(app.experiments.voice, "Dictate into the composer", { store.updateExperiments { it.copy(voice = !app.experiments.voice) } }, checkedState = "On", uncheckedState = "Off")
                        Label("Dictate into the composer")
                    }
                    Label("On Android and the web, audio may go to Google.", Modifier.padding(start = Tokens.Space.S8 + Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted)
                }
            }

            VoiceStep("2", "How speech becomes text") {
                if (choosable) {
                    Segmented(listOf(OnTheServer, ThisDevice), if (speechChoice == SpeechToTextDevice) ThisDevice else OnTheServer, { picked -> store.voice.setSpeechToText(if (picked == ThisDevice) SpeechToTextDevice else SpeechToTextProvider) }, groupLabel = "Speech to text")
                }

                if (speechChoice == SpeechToTextDevice) {
                    Label(DeviceRecogniserWarning, style = Tokens.Type.Meta, color = colors.textMuted)
                } else if (!privacy.hasSpeechToText()) {
                    FailureBlock {
                        Label(NoSpeechProviderStatement, style = Tokens.Type.Meta)
                        Row { MyButton({ changing = SpeechToTextUse }, text = "Choose speech to text", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
                    }
                } else {
                    SettingRow("Model", narrow, {
                        if (local != null) {
                            Label("Local model")
                            Label(local.modelId, style = Tokens.Type.Code, color = colors.textMuted)

                            when (local.state) {
                                DownloadingState -> {
                                    val total = local.totalBytes

                                    if (total != null && total > 0) {
                                        DeterminateProgress("Downloading ${local.modelId}", local.downloadedBytes.toFloat() / total, Modifier.padding(top = Tokens.Space.S1_5))
                                        Label("Downloading, ${megabytes(local.downloadedBytes)} of ${megabytes(total)}", style = Tokens.Type.Meta, color = colors.textMuted)
                                    } else {
                                        IndeterminateProgress("Downloading ${local.modelId}", Modifier.padding(top = Tokens.Space.S1_5))
                                        Label("Downloading, ${megabytes(local.downloadedBytes)}", style = Tokens.Type.Meta, color = colors.textMuted)
                                    }
                                }

                                ReadyState -> Label("Ready, ${megabytes(local.downloadedBytes)} on the server", style = Tokens.Type.Meta, color = colors.textMuted)
                            }
                        } else if (provider != null) {
                            Label(provider.name)
                            Label(provider.model, style = Tokens.Type.Code, color = colors.textMuted)
                        }
                    }) {
                        if (recorder != null && local?.state != DownloadingState && local?.state != FailedState) {
                            MyButton(::toggleRecording, text = if (recording) "Stop" else "Try it", icon = if (recording) MyIcon.Stop else MyIcon.Microphone, description = if (recording) "Stop speech to text" else "Try speech to text", size = ButtonSize.Sm, muted = true)
                        }

                        MyButton({ changing = SpeechToTextUse }, text = "Change", description = "Change speech to text", size = ButtonSize.Sm, muted = true)
                    }

                    if (local?.state == FailedState) {
                        FailureBlock {
                            Label("Could not download ${local.modelId}: ${local.failure ?: "The download stopped."}", style = Tokens.Type.Meta)
                            Row { MyButton({ dictationRunner { store.voice.chooseLocalModel(local.modelId) } }, text = "Try again", description = "Download ${local.modelId} again", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
                        }
                    }

                    heard?.let { words ->
                        StatusBlock {
                            Column(Modifier.weight(1f)) {
                                Label("Heard", style = Tokens.Type.Meta, color = colors.textMuted)
                                Label(words)
                            }
                        }
                    }

                    dictationRunner.failure?.let { ErrorBanner(it, onDismiss = { dictationRunner.failure = null }) }
                }
            }

            VoiceStep("3", "How replies are spoken") {
                Segmented(
                    listOf(YourProvider, ReadAloudOffName),
                    if (readsAloudOff) ReadAloudOffName else YourProvider,
                    { picked -> if (speaker != null) store.voice.setReadAloud(if (picked == ReadAloudOffName) ReadAloudOff else ReadAloudProvider) },
                    groupLabel = "Reading replies aloud",
                )

                if (speaker == null) {
                    Label(NoTextToSpeechStatement, style = Tokens.Type.Meta, color = colors.textMuted)

                    if (openAiCompatible.isNotEmpty()) {
                        Row { MyButton({ changing = "text_to_speech" }, text = "Choose a model", description = "Choose a text-to-speech model from your providers", size = ButtonSize.Sm, muted = true) }
                    } else {
                        Row { MyButton({ store.openScreen("settings/providers/$AddSpeechProviderAnchor") }, text = "Add a provider", icon = MyIcon.Plus, description = "Add a provider for text to speech", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
                    }
                } else {
                    SettingRow("Model", narrow, {
                        Label(speaker.name)
                        Label(speaker.model, style = Tokens.Type.Code, color = colors.textMuted)
                    }) {
                        MyButton(::readSampleAloud, text = "Try it", icon = MyIcon.Play, description = "Try reading aloud", size = ButtonSize.Sm, muted = true)
                        MyButton({ changing = "text_to_speech" }, text = "Change", description = "Change reading aloud", size = ButtonSize.Sm, muted = true)
                    }

                    if (assistant != null) {
                        SettingRow("Voice", narrow, {
                            Row {
                                OptionMenu("Voice", voice.voices.map { Choice(it, it) }, assistant.preferences.voiceName, { picked -> savePreferences { it.copy(voiceName = picked) } }, title = "Voice", label = "Provider default", size = ButtonSize.Sm, variant = ButtonVariant.Outline)
                            }
                        })

                        SettingRow("Speaking speed", narrow, {
                            val speed = assistant.preferences.speechSpeed ?: SpeechSpeedNormal

                            Segmented(SpeechSpeeds.map { it.second }, SpeechSpeeds.first { it.first == speed }.second, { picked -> savePreferences { it.copy(speechSpeed = SpeechSpeeds.first { speed -> speed.second == picked }.first) } }, groupLabel = "Speaking speed")
                        })
                    }

                    speakingRunner.failure?.let { ErrorBanner(it, onDismiss = { speakingRunner.failure = null }) }
                    preferencesRunner.failure?.let { ErrorBanner(it, onDismiss = { preferencesRunner.failure = null }) }
                }
            }

            VoiceStep("4", "What leaves this machine") {
                val speech = when {
                    speechChoice == SpeechToTextDevice -> DeviceRecogniserWarning to false
                    local != null -> "Sent to your Mycelium server and turned into text there. It goes no further." to true
                    provider != null -> "Sent to ${hostOf(provider, providers)} to turn into text." to false
                    else -> "Nothing is sent until speech to text is chosen." to true
                }
                val replies = if (readsAloudOff || speaker == null) "Off." to true else "Reply text is sent to ${hostOf(speaker, providers)}." to false
                val rows = listOf(
                    Triple("Wake phrase", "Stays on this device.", true),
                    Triple("Your speech", speech.first, speech.second),
                    Triple("Replies read aloud", replies.first, replies.second),
                )

                Column(Modifier.fillMaxWidth().clip(BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape)) {
                    for ((index, row) in rows.withIndex()) {
                        if (index > 0) {
                            Hairline()
                        }

                        if (narrow) {
                            Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                                Label(row.first, style = Tokens.Type.UiStrong)
                                Label(row.second, color = if (row.third) colors.textMuted else colors.text)
                            }
                        } else {
                            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)) {
                                Label(row.first, Modifier.width(FactNameWidth))
                                Label(row.second, Modifier.weight(1f), color = if (row.third) colors.textMuted else colors.text)
                            }
                        }
                    }
                }

                Label("No audio is stored.", style = Tokens.Type.Meta, color = colors.textMuted)
            }
        }
    }

    uses.firstOrNull { it.id == changing }?.let { use ->
        SpeechModelDialog(store, use, openAiCompatible, local, { changing = null }) {
            changing = null
            store.openScreen("settings/providers/$AddSpeechProviderAnchor")
        }
    }
}

@Composable
private fun VoiceStep(number: String, title: String, content: @Composable () -> Unit) {
    val colors = MyTheme.colors

    Column(Modifier.fillMaxWidth().clip(BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape)) {
        Row(Modifier.fillMaxWidth().background(colors.bgPanel).padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label(number, Modifier.clearAndSetSemantics {}, Tokens.Type.Code, colors.textMuted)
            Label(title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        }

        Hairline()

        Column(Modifier.fillMaxWidth().padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            content()
        }
    }
}

@Composable
private fun SettingRow(name: String, narrow: Boolean, state: @Composable () -> Unit, actions: @Composable () -> Unit = {}) {
    val colors = MyTheme.colors

    if (narrow) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label(name, style = Tokens.Type.Meta, color = colors.textMuted)

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
                Column(Modifier.weight(1f)) { state() }
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) { actions() }
            }
        }
    } else {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
            Label(name, Modifier.width(SettingNameWidth).padding(top = Tokens.Space.S0_5), color = colors.textMuted)
            Column(Modifier.weight(1f)) { state() }
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) { actions() }
        }
    }
}

@Composable
private fun StatusBlock(content: @Composable RowScope.() -> Unit) {
    val colors = MyTheme.colors

    Row(
        Modifier
            .fillMaxWidth()
            .background(colors.bgPanel, BlockShape)
            .border(Tokens.Size.Hairline, colors.border, BlockShape)
            .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        content = content,
    )
}

@Composable
private fun SpeechModelDialog(store: Store, use: SpeechUse, providers: List<ModelProvider>, localModel: LocalSpeechModel?, onDismiss: () -> Unit, onAddProvider: () -> Unit) {
    val colors = MyTheme.colors
    val start = providers.firstOrNull { it.id == use.chosen?.providerId } ?: providers.firstOrNull { use.recognised(it).isNotEmpty() } ?: providers.firstOrNull()
    var usesLocal by remember { mutableStateOf(use.local && localModel != null) }
    var providerId by remember { mutableStateOf(start?.id) }
    val provider = providers.firstOrNull { it.id == providerId }
    val recognised = provider?.let(use.recognised).orEmpty()
    val others = provider?.models.orEmpty().filter { it !in recognised }
    var model by remember { mutableStateOf(use.chosen?.takeIf { it.providerId == start?.id }?.model ?: recognised.firstOrNull()) }
    var modelId by remember { mutableStateOf(localModel?.modelId.orEmpty()) }
    var modelIdError by remember { mutableStateOf<String?>(null) }
    val runner = rememberAction()
    val title = if (use.local) "Speech to text" else use.name

    fun save() {
        if (usesLocal) {
            if (modelId.isBlank()) {
                modelIdError = EmptyLocalModelStatement

                return
            }

            runner {
                store.voice.chooseLocalModel(modelId.trim())
                onDismiss()
            }

            return
        }

        val chosenProvider = provider ?: return
        val chosenModel = model

        if (chosenModel == null) {
            runner.failure = "Select a model for ${use.name.lowercase()}."

            return
        }

        runner {
            store.voice.chooseSpeechModel(use.id, chosenProvider.id, chosenModel)
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Label(title, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
            MyButton(onDismiss, icon = MyIcon.Close, description = "Close", size = ButtonSize.Sm, muted = true)
        }

        if (use.local) {
            Segmented(listOf("A provider", "A local model"), if (usesLocal) "A local model" else "A provider", { picked -> usesLocal = picked == "A local model"; runner.failure = null }, groupLabel = "Turned into text by")
        }

        if (usesLocal) {
            TextInput(
                modelId,
                { modelId = it; modelIdError = null; runner.failure = null },
                label = "Hugging Face model",
                placeholder = "owner/name",
                error = modelIdError ?: runner.failure,
                mono = true,
                onSubmit = ::save,
                hint = "Enter the model id, as owner/name. The server downloads it the first time and turns speech into text there, so audio never leaves the server.",
            )

            StatusBlock {
                Label("The model's own code runs on your server. Only use a model you trust.", Modifier.weight(1f), Tokens.Type.Meta)
            }

            if (localModel != null) {
                Row { MyButton({ runner { store.voice.removeLocalModel(); onDismiss() } }, text = "Remove the downloaded model", size = ButtonSize.Sm, muted = true) }
            }
        } else if (providers.isEmpty()) {
            Label("No provider is set up for ${use.name.lowercase()}.", color = colors.textMuted)
            Row { MyButton(onAddProvider, text = "Add a provider", icon = MyIcon.Plus, description = "Add a provider for ${use.name.lowercase()}", variant = ButtonVariant.Outline) }
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Provider")
                Row {
                    OptionMenu("Provider", providers.map { Choice(it.id, it.name) }, providerId, { picked ->
                        providerId = picked
                        model = providers.firstOrNull { it.id == picked }?.let(use.recognised)?.firstOrNull()
                        runner.failure = null
                    }, variant = ButtonVariant.Outline)
                }

                if (provider != null && provider.models.isEmpty()) {
                    Label("${provider.name} offers no models. Select another provider, or refresh its models in Chat models.", style = Tokens.Type.Meta, color = colors.statusFailed)
                }
            }

            if (provider != null && provider.models.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Label("Model")
                    Row {
                        OptionMenu(
                            "Model",
                            recognised.map { Choice(it, it) } + others.map { Choice(it, it, group = OtherModels.takeIf { recognised.isNotEmpty() }) },
                            model,
                            { model = it; runner.failure = null },
                            title = if (recognised.isNotEmpty()) use.recognisedTitle else OtherModels,
                            variant = ButtonVariant.Outline,
                        )
                    }
                }
            }
        }

        if (!usesLocal) {
            runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = if (usesLocal) "Download and use" else "Save", variant = ButtonVariant.Primary)
        }
    }
}
