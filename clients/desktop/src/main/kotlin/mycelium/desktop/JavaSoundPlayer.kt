package mycelium.desktop

import java.io.ByteArrayInputStream
import javax.sound.sampled.AudioSystem
import javax.sound.sampled.Clip
import javax.sound.sampled.LineEvent
import mycelium.AudioPlayer

class JavaSoundPlayer : AudioPlayer {
    private var clip: Clip? = null

    override fun play(audio: ByteArray, mediaType: String, onDone: (Boolean) -> Unit) {
        val opened = try {
            AudioSystem.getClip().also { held ->
                held.open(AudioSystem.getAudioInputStream(ByteArrayInputStream(audio)))
            }
        } catch (_: Exception) {
            null
        }

        if (opened == null) {
            onDone(false)
            return
        }

        clip = opened
        opened.addLineListener { event ->
            if (event.type == LineEvent.Type.STOP) {
                opened.close()
                onDone(true)
            }
        }
        opened.start()
    }

    override fun stop() {
        clip?.stop()
        clip?.close()
        clip = null
    }
}
