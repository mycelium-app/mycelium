package mycelium

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.runCurrent
import kotlin.test.Test
import kotlin.test.assertEquals

private class SilentRecorder : AudioRecorder {
    override fun start(onStopped: (DictationProblem?) -> Unit) = Unit

    override suspend fun stop(): ByteArray = byteArrayOf(1)
}

@OptIn(ExperimentalCoroutinesApi::class)
class ProviderDictationTest {
    @Test
    fun aNetworkFailureStillStopsListeningAndSaysWhatWentWrong() = runTest {
        val dictation = ProviderDictation(SilentRecorder(), { error("connection reset") }, backgroundScope)
        val stops = mutableListOf<DictationProblem?>()

        dictation.start({}, { stops += it })
        dictation.stop()
        runCurrent()

        assertEquals(listOf<DictationProblem?>(DictationProblem.Failed("Could not reach the control plane. Check your connection and try again.")), stops)
    }
}
