package mycelium.state

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import mycelium.AudioPlayer
import mycelium.Dictation
import mycelium.DictationProblem
import mycelium.Platform
import mycelium.ProviderDictation
import mycelium.api.ApiException
import mycelium.api.MyceliumApi
import mycelium.api.PrivacySettings
import mycelium.api.SavePrivacyRequest
import mycelium.api.SpeechRequest

const val VoiceAssistantKey = "voice"
const val WakePhraseHey = "hey_mycelium"
const val WakePhraseOk = "ok_mycelium"
const val SpeechToTextProvider = "provider"
const val SpeechToTextDevice = "device"
const val ReadAloudProvider = "provider"
const val ReadAloudOff = "off"
const val SpeechSpeedNormal = "normal"

private const val WakePhraseEnabledKey = "voice-wake-phrase-enabled"
private const val WakePhraseKey = "voice-wake-phrase"
private const val SpeechToTextKey = "voice-speech-to-text"
private const val ReadAloudKey = "voice-read-aloud"
private const val LevelPollMillis = 100L
private const val PauseMillis = 1_500L
private const val HeardLevel = 0.08f

sealed interface VoiceProblem {
    data object MicrophoneDenied : VoiceProblem

    data object NoMicrophone : VoiceProblem

    data object NoSpeechProvider : VoiceProblem

    data class Failed(val message: String) : VoiceProblem
}

sealed interface VoicePhase {
    data object Idle : VoicePhase

    data object Listening : VoicePhase

    data object Thinking : VoicePhase

    data object Speaking : VoicePhase

    data object Ended : VoicePhase

    data class Problem(val problem: VoiceProblem) : VoicePhase
}

data class VoiceDevice(
    val wakePhraseEnabled: Boolean = false,
    val wakePhrase: String = WakePhraseHey,
    val speechToText: String? = null,
    val readAloud: String = ReadAloudProvider,
)

data class VoiceTurn(val said: String = "", val tool: String? = null, val reply: String = "", val conversationId: String? = null, val readingFailed: Boolean = false)

data class VoiceState(
    val privacy: PrivacySettings = PrivacySettings(),
    val loaded: Boolean = false,
    val open: Boolean = false,
    val phase: VoicePhase = VoicePhase.Idle,
    val heard: String = "",
    val level: Float = 0f,
    val turn: VoiceTurn = VoiceTurn(),
    val voices: List<String> = emptyList(),
    val device: VoiceDevice = VoiceDevice(),
)

fun wakePhraseName(phrase: String): String = if (phrase == WakePhraseOk) "OK Mycelium" else "Hey Mycelium"

fun voiceProblemOf(problem: Throwable): VoiceProblem = when {
    problem is ApiException && problem.code == "no_speech_provider" -> VoiceProblem.NoSpeechProvider
    problem is ApiException -> VoiceProblem.Failed(problem.message)
    else -> VoiceProblem.Failed("Could not reach the control plane. Check your connection and try again.")
}

class VoiceStore(private val api: MyceliumApi, private val scope: CoroutineScope, private val platform: Platform?) {
    private val mutableState = MutableStateFlow(VoiceState(device = readDevice()))
    private var dictation: Dictation? = null
    private var player: AudioPlayer? = null
    private var meter: Job? = null
    private var discarded = false
    private var openVoiceThread: () -> Unit = {}
    private var voiceName: String? = null
    private var speechSpeed: String? = null

    val state: StateFlow<VoiceState> = mutableState.asStateFlow()

    private fun readDevice(): VoiceDevice = VoiceDevice(
        wakePhraseEnabled = platform?.loadPreference(WakePhraseEnabledKey) == true.toString(),
        wakePhrase = platform?.loadPreference(WakePhraseKey) ?: WakePhraseHey,
        speechToText = platform?.loadPreference(SpeechToTextKey),
        readAloud = platform?.loadPreference(ReadAloudKey) ?: ReadAloudProvider,
    )

    fun speechToTextChoice(): String {
        val chosen = mutableState.value.device.speechToText
        val deviceAvailable = platform?.dictation() != null
        val providerAvailable = platform?.audioRecorder() != null

        return when {
            !deviceAvailable -> SpeechToTextProvider
            !providerAvailable -> SpeechToTextDevice
            chosen != null -> chosen
            mutableState.value.privacy.speechToTextProvider == null -> SpeechToTextDevice
            else -> SpeechToTextProvider
        }
    }

    suspend fun load() {
        val privacy = try {
            api.privacy()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            return
        }

        mutableState.update { it.copy(privacy = privacy, loaded = true) }
    }

    suspend fun loadVoices() {
        val voices = try {
            api.speechVoices()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (_: Exception) {
            return
        }

        mutableState.update { it.copy(voices = voices) }
    }

    fun useVoice(name: String?, speed: String?) {
        voiceName = name
        speechSpeed = speed
    }

    fun onOpenThread(open: () -> Unit) {
        openVoiceThread = open
    }

    fun openThread() {
        openVoiceThread()
    }

    fun setEnabled(on: Boolean) {
        val before = mutableState.value.privacy

        mutableState.update { it.copy(privacy = it.privacy.copy(voiceEnabled = on)) }

        if (!on) {
            close()
        }

        scope.launch {
            val saved = try {
                api.savePrivacy(SavePrivacyRequest(on))
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                null
            }

            mutableState.update { it.copy(privacy = saved ?: before) }
        }
    }

    fun setWakePhraseEnabled(on: Boolean) {
        platform?.savePreference(WakePhraseEnabledKey, on.toString())
        mutableState.update { it.copy(device = it.device.copy(wakePhraseEnabled = on)) }
    }

    fun setWakePhrase(phrase: String) {
        platform?.savePreference(WakePhraseKey, phrase)
        mutableState.update { it.copy(device = it.device.copy(wakePhrase = phrase)) }
    }

    fun setSpeechToText(choice: String) {
        platform?.savePreference(SpeechToTextKey, choice)
        mutableState.update { it.copy(device = it.device.copy(speechToText = choice)) }
    }

    fun setReadAloud(choice: String) {
        platform?.savePreference(ReadAloudKey, choice)
        mutableState.update { it.copy(device = it.device.copy(readAloud = choice)) }
    }

    fun readsAloud(): Boolean {
        val current = mutableState.value

        return current.device.readAloud != ReadAloudOff && current.privacy.textToSpeechProvider != null
    }

    fun toggleCard() {
        val current = mutableState.value

        if (current.open) {
            close()
        } else {
            mutableState.update { it.copy(open = true) }
        }
    }

    fun toggleListening() {
        when (mutableState.value.phase) {
            VoicePhase.Listening -> stopListening()
            VoicePhase.Thinking -> Unit
            else -> startListening()
        }
    }

    fun startListening() {
        stopSpeaking()
        discarded = false

        val choice = speechToTextChoice()
        val noProvider = choice == SpeechToTextProvider && mutableState.value.privacy.speechToTextProvider == null
        val route = when {
            choice == SpeechToTextDevice -> platform?.dictation()
            noProvider -> null
            else -> platform?.audioRecorder()?.let { ProviderDictation(it, api::transcribe, scope) }
        }

        if (route == null) {
            val problem = if (noProvider) VoiceProblem.NoSpeechProvider else VoiceProblem.NoMicrophone

            mutableState.update { it.copy(open = true, phase = VoicePhase.Problem(problem)) }
            return
        }

        dictation = route
        mutableState.update { it.copy(open = true, phase = VoicePhase.Listening, heard = "", level = 0f, turn = VoiceTurn()) }
        route.start({ words -> mutableState.update { it.copy(heard = "${it.heard} $words".trim()) } }, { problem -> stopped(problem) })
        watchLevel(route)
    }

    private fun watchLevel(route: Dictation) {
        meter?.cancel()
        meter = scope.launch {
            var spoke = false
            var quietFor = 0L

            while (mutableState.value.phase == VoicePhase.Listening) {
                val level = route.level()

                mutableState.update { it.copy(level = level) }

                if (level >= HeardLevel) {
                    spoke = true
                    quietFor = 0
                } else if (spoke) {
                    quietFor += LevelPollMillis
                }

                if (spoke && quietFor >= PauseMillis) {
                    stopListening()
                    return@launch
                }

                delay(LevelPollMillis)
            }
        }
    }

    private fun stopped(problem: DictationProblem?) {
        meter?.cancel()

        val current = mutableState.value

        if (current.phase != VoicePhase.Listening) {
            return
        }

        if (problem != null) {
            val shown = when (problem) {
                DictationProblem.Denied -> VoiceProblem.MicrophoneDenied
                DictationProblem.NoMicrophone -> VoiceProblem.NoMicrophone
                is DictationProblem.Failed -> if (problem.message.contains("speech-to-text model")) VoiceProblem.NoSpeechProvider else VoiceProblem.Failed(problem.message)
            }

            mutableState.update { it.copy(phase = VoicePhase.Problem(shown)) }
            return
        }

        val said = current.heard.trim()

        if (discarded || said.isEmpty()) {
            mutableState.update { it.copy(phase = VoicePhase.Idle, heard = "", level = 0f) }
            return
        }

        send(said)
    }

    fun stopListening() {
        stopRecording()
    }

    fun cancelListening() {
        discarded = true
        meter?.cancel()
        stopRecording()
        mutableState.update { it.copy(phase = VoicePhase.Idle, heard = "", level = 0f) }
    }

    private fun stopRecording() {
        if (mutableState.value.phase == VoicePhase.Listening) {
            dictation?.stop()
        }
    }

    private fun send(said: String) {
        mutableState.update { it.copy(phase = VoicePhase.Thinking, level = 0f, turn = VoiceTurn(said = said)) }
        scope.launch {
            try {
                val assistant = api.voiceAssistant()
                val conversation = api.promptAssistant(assistant.id, said, spoken = true)

                mutableState.update { it.copy(turn = it.turn.copy(conversationId = conversation.id)) }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: Exception) {
                mutableState.update { it.copy(phase = VoicePhase.Problem(voiceProblemOf(problem))) }
            }
        }
    }

    fun observe(app: AppState) {
        val current = mutableState.value
        val id = current.turn.conversationId ?: return

        if (current.phase != VoicePhase.Thinking) {
            return
        }

        val events = app.events[id].orEmpty()
        val items = ThreadBuilder().build(events)
        val asked = items.indexOfLast { it is ThreadItem.UserMessage }

        if (asked < 0 || (items[asked] as ThreadItem.UserMessage).text.trim() != current.turn.said) {
            return
        }

        val after = items.drop(asked + 1)
        val reply = after.filterIsInstance<ThreadItem.AgentMessage>().joinToString("\n\n") { it.text.trim() }
        val tool = after.filterIsInstance<ThreadItem.ToolCall>().lastOrNull { it.status == ToolStatus.Running }?.title

        mutableState.update { it.copy(turn = it.turn.copy(reply = reply, tool = tool)) }

        if (!turnIsOpen(events) && reply.isNotBlank()) {
            finish(reply)
        }
    }

    private fun finish(reply: String) {
        if (!readsAloud()) {
            mutableState.update { it.copy(phase = VoicePhase.Ended) }
            return
        }

        mutableState.update { it.copy(phase = VoicePhase.Speaking) }
        speak(reply) { mutableState.update { state -> if (state.phase == VoicePhase.Speaking) state.copy(phase = VoicePhase.Ended) else state } }
    }

    fun readAloud(text: String) {
        mutableState.update { it.copy(open = true, phase = VoicePhase.Speaking, turn = it.turn.copy(reply = text)) }
        speak(text) { mutableState.update { state -> if (state.phase == VoicePhase.Speaking) state.copy(phase = VoicePhase.Ended) else state } }
    }

    private fun speak(text: String, onDone: () -> Unit) {
        val output = platform?.audioPlayer()

        if (output == null) {
            mutableState.update { it.copy(turn = it.turn.copy(readingFailed = true)) }
            onDone()
            return
        }

        scope.launch {
            try {
                val audio = api.speech(SpeechRequest(text, voiceName, speechSpeed))

                if (mutableState.value.phase != VoicePhase.Speaking) {
                    return@launch
                }

                player = output
                output.play(audio.bytes, audio.mediaType) { played ->
                    if (!played) {
                        mutableState.update { it.copy(turn = it.turn.copy(readingFailed = true)) }
                    }

                    onDone()
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                mutableState.update { it.copy(turn = it.turn.copy(readingFailed = true)) }
                onDone()
            }
        }
    }

    fun stopSpeaking() {
        player?.stop()
        player = null

        mutableState.update { if (it.phase == VoicePhase.Speaking) it.copy(phase = VoicePhase.Ended) else it }
    }

    fun stopTurn() {
        val id = mutableState.value.turn.conversationId

        mutableState.update { it.copy(phase = VoicePhase.Idle) }

        if (id != null) {
            scope.launch { catching { api.cancel(id) } }
        }
    }

    fun typeInstead() {
        close()
        openVoiceThread()
    }

    fun close() {
        discarded = true
        meter?.cancel()
        stopRecording()
        stopSpeaking()
        mutableState.update { it.copy(open = false, phase = VoicePhase.Idle, heard = "", level = 0f) }
    }
}
