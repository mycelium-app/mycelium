package mycelium.android

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import androidx.core.content.ContextCompat
import mycelium.Dictation
import mycelium.DictationProblem

private const val Stopped = "Dictation stopped. Try again."

class AndroidDictation(private val context: Context, private val requestPermission: ((Boolean) -> Unit) -> Unit) : Dictation {
    private var recogniser: SpeechRecognizer? = null

    override fun start(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) {
        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

        if (granted) {
            listen(onHeard, onStopped)
        } else {
            requestPermission { allowed ->
                if (allowed) listen(onHeard, onStopped) else onStopped(DictationProblem.Denied)
            }
        }
    }

    override fun stop() {
        recogniser?.stopListening()
    }

    private fun listen(onHeard: (String) -> Unit, onStopped: (DictationProblem?) -> Unit) {
        val created = SpeechRecognizer.createSpeechRecognizer(context)
        recogniser = created

        fun finish(problem: DictationProblem?) {
            created.destroy()

            if (recogniser === created) {
                recogniser = null
            }

            onStopped(problem)
        }

        created.setRecognitionListener(object : RecognitionListener {
            override fun onResults(results: Bundle?) {
                results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull()?.let(onHeard)
                finish(null)
            }

            override fun onError(error: Int) {
                when (error) {
                    SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> finish(DictationProblem.Denied)
                    SpeechRecognizer.ERROR_NO_MATCH, SpeechRecognizer.ERROR_SPEECH_TIMEOUT -> finish(null)
                    else -> finish(DictationProblem.Failed(Stopped))
                }
            }

            override fun onReadyForSpeech(params: Bundle?) = Unit

            override fun onBeginningOfSpeech() = Unit

            override fun onRmsChanged(rmsdB: Float) = Unit

            override fun onBufferReceived(buffer: ByteArray?) = Unit

            override fun onEndOfSpeech() = Unit

            override fun onPartialResults(partialResults: Bundle?) = Unit

            override fun onEvent(eventType: Int, params: Bundle?) = Unit
        })
        created.startListening(
            Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
                .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
                .putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, false),
        )
    }
}
