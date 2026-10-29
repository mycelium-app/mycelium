package mycelium

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import mycelium.api.ApiException

sealed class DictationProblem {
    data object Denied : DictationProblem()

    data object NoMicrophone : DictationProblem()

    data class Failed(val message: String) : DictationProblem()
}

interface Dictation {
    fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit)

    fun stop()

    fun level(): Float = 0f
}

interface AudioRecorder {
    fun start(onStopped: (DictationProblem?) -> Unit)

    suspend fun stop(): ByteArray

    fun level(): Float = 0f
}

interface AudioPlayer {
    fun play(audio: ByteArray, mediaType: String, onDone: (Boolean) -> Unit)

    fun stop()
}

interface WakeListener {
    fun start(phrase: String, onHeard: () -> Unit): Boolean

    fun stop()
}

class ProviderDictation(private val recorder: AudioRecorder, private val transcribe: suspend (ByteArray) -> String, private val scope: CoroutineScope) : Dictation {
    private var onHeard: (String) -> Unit = {}
    private var onStopped: (DictationProblem?) -> Unit = {}

    override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) {
        this.onHeard = onHeard
        this.onStopped = onStopped
        recorder.start { problem -> onStopped(problem) }
    }

    override fun level(): Float = recorder.level()

    override fun stop() {
        val heard = onHeard
        val stopped = onStopped

        scope.launch {
            try {
                val audio = recorder.stop()

                heard(transcribe(audio))
                stopped(null)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: ApiException) {
                stopped(DictationProblem.Failed(problem.message))
            } catch (_: Exception) {
                stopped(DictationProblem.Failed("Could not reach the control plane. Check your connection and try again."))
            }
        }
    }
}
