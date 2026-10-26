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
        val wav = ByteArrayOutputStream()
        AudioSystem.write(AudioInputStream(ByteArrayInputStream(pcm), format, (pcm.size / format.frameSize).toLong()), AudioFileFormat.Type.WAVE, wav)

        wav.toByteArray()
    }
}
