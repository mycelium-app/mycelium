package mycelium.android

import android.media.MediaDataSource
import android.media.MediaPlayer
import mycelium.AudioPlayer

private class BytesSource(private val bytes: ByteArray) : MediaDataSource() {
    override fun readAt(position: Long, buffer: ByteArray, offset: Int, size: Int): Int {
        if (position >= bytes.size) {
            return -1
        }

        val count = minOf(size, bytes.size - position.toInt())

        bytes.copyInto(buffer, offset, position.toInt(), position.toInt() + count)

        return count
    }

    override fun getSize(): Long = bytes.size.toLong()

    override fun close() = Unit
}

class AndroidAudioPlayer : AudioPlayer {
    private var player: MediaPlayer? = null

    override fun play(audio: ByteArray, mediaType: String, onDone: (Boolean) -> Unit) {
        val held = MediaPlayer()

        player = held
        held.setDataSource(BytesSource(audio))
        held.setOnCompletionListener {
            release()
            onDone(true)
        }
        held.setOnErrorListener { _, _, _ ->
            release()
            onDone(false)
            true
        }
        held.setOnPreparedListener { it.start() }
        held.prepareAsync()
    }

    override fun stop() {
        release()
    }

    private fun release() {
        player?.release()
        player = null
    }
}
