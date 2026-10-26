package mycelium

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
}

interface AudioRecorder {
    fun start(onStopped: (DictationProblem?) -> Unit)

    suspend fun stop(): ByteArray
}

class ProviderDictation(private val recorder: AudioRecorder, private val transcribe: suspend (ByteArray) -> String, private val scope: CoroutineScope) : Dictation {
    private var onHeard: (String) -> Unit = {}
    private var onStopped: (DictationProblem?) -> Unit = {}

    override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) {
        this.onHeard = onHeard
        this.onStopped = onStopped
        recorder.start { problem -> onStopped(problem) }
    }

    override fun stop() {
        val heard = onHeard
        val stopped = onStopped

        scope.launch {
            val audio = recorder.stop()

            try {
                heard(transcribe(audio))
                stopped(null)
            } catch (problem: ApiException) {
                stopped(DictationProblem.Failed(problem.message))
            }
        }
    }
}
