package mycelium.desktop

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import javax.sound.sampled.AudioFileFormat
import javax.sound.sampled.AudioFormat
import javax.sound.sampled.AudioInputStream
import javax.sound.sampled.AudioSystem
import javax.sound.sampled.LineUnavailableException
import javax.sound.sampled.TargetDataLine
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import mycelium.AudioRecorder
import mycelium.DictationProblem

private const val SampleRate = 16000f
private const val BytesPerSecond = 32000
private const val MaxRecordingSeconds = 600
private const val ReadChunkBytes = 4096

class JavaSoundRecorder : AudioRecorder {
    private val format = AudioFormat(SampleRate, 16, 1, true, false)
    private val captured = ByteArrayOutputStream()
    private var line: TargetDataLine? = null
    private var reader: Thread? = null

    @Volatile
    private var heard = 0f

    override fun level(): Float = heard

    override fun start(onStopped: (DictationProblem?) -> Unit) {
        captured.reset()

        val opened = try {
            AudioSystem.getTargetDataLine(format).also {
                it.open(format)
                it.start()
            }
        } catch (problem: LineUnavailableException) {
            null
        } catch (problem: IllegalArgumentException) {
            null
        } catch (problem: SecurityException) {
            null
        }

        if (opened == null) {
            onStopped(DictationProblem.NoMicrophone)
            return
        }

        line = opened
        reader = Thread {
            val chunk = ByteArray(ReadChunkBytes)

            while (opened.isOpen && captured.size() < MaxRecordingSeconds * BytesPerSecond) {
                val read = opened.read(chunk, 0, chunk.size)

                if (read > 0) {
                    captured.write(chunk, 0, read)
                    heard = loudness(chunk, read)
                }
            }
        }.apply {
            isDaemon = true
            start()
        }
    }

    override suspend fun stop(): ByteArray = withContext(Dispatchers.IO) {
        line?.stop()
        line?.close()
        reader?.join()
        line = null
        reader = null

        val pcm = captured.toByteArray()
        captured.reset()
        heard = 0f
        val wav = ByteArrayOutputStream()
        AudioSystem.write(AudioInputStream(ByteArrayInputStream(pcm), format, (pcm.size / format.frameSize).toLong()), AudioFileFormat.Type.WAVE, wav)

        wav.toByteArray()
    }
}

private fun loudness(chunk: ByteArray, length: Int): Float {
    var sum = 0.0
    var samples = 0
    var index = 0

    while (index + 1 < length) {
        val sample = ((chunk[index + 1].toInt() shl 8) or (chunk[index].toInt() and 0xFF)).toShort().toInt()

        sum += sample.toDouble() * sample
        samples += 1
        index += 2
    }

    if (samples == 0) {
        return 0f
    }

    return (kotlin.math.sqrt(sum / samples) / Short.MAX_VALUE * LoudnessGain).toFloat().coerceIn(0f, 1f)
}

private const val LoudnessGain = 6.0
