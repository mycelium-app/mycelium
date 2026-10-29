package mycelium.desktop

import com.k2fsa.sherpa.onnx.FeatureConfig
import com.k2fsa.sherpa.onnx.KeywordSpotter
import com.k2fsa.sherpa.onnx.KeywordSpotterConfig
import com.k2fsa.sherpa.onnx.OnlineModelConfig
import com.k2fsa.sherpa.onnx.OnlineTransducerModelConfig
import java.nio.file.Path
import javax.sound.sampled.AudioFormat
import javax.sound.sampled.AudioSystem
import javax.sound.sampled.TargetDataLine
import mycelium.WakeListener

private const val SampleRate = 16000
private const val FeatureDimension = 80
private const val ChunkBytes = 3200
private const val ModelSuffix = "epoch-12-avg-2-chunk-16-left-64.int8.onnx"

fun openWakeSpotter(modelDirectory: Path): KeywordSpotter {
    val transducer = OnlineTransducerModelConfig.builder()
        .setEncoder(modelDirectory.resolve("encoder-$ModelSuffix").toString())
        .setDecoder(modelDirectory.resolve("decoder-$ModelSuffix").toString())
        .setJoiner(modelDirectory.resolve("joiner-$ModelSuffix").toString())
        .build()
    val model = OnlineModelConfig.builder()
        .setTransducer(transducer)
        .setTokens(modelDirectory.resolve("tokens.txt").toString())
        .setNumThreads(1)
        .setProvider("cpu")
        .build()
    val config = KeywordSpotterConfig.builder()
        .setOnlineModelConfig(model)
        .setFeatureConfig(FeatureConfig.builder().setSampleRate(SampleRate).setFeatureDim(FeatureDimension).build())
        .setKeywordsFile(modelDirectory.resolve("keywords.txt").toString())
        .build()

    return KeywordSpotter(config)
}

class SherpaWakeListener(private val modelDirectory: Path) : WakeListener {
    private val format = AudioFormat(SampleRate.toFloat(), 16, 1, true, false)

    @Volatile
    private var line: TargetDataLine? = null

    @Volatile
    private var worker: Thread? = null

    override fun start(phrase: String, onHeard: () -> Unit): Boolean {
        stop()

        val spotter = runCatching { openWakeSpotter(modelDirectory) }.getOrNull() ?: return false
        val opened = runCatching { AudioSystem.getTargetDataLine(format).also { it.open(format); it.start() } }.getOrNull()

        if (opened == null) {
            spotter.release()
            return false
        }

        line = opened
        worker = Thread {
            val heard = listenFor(phrase, spotter, opened)

            opened.close()
            spotter.release()

            if (heard) {
                onHeard()
            }
        }.apply {
            isDaemon = true
            start()
        }

        return true
    }

    override fun stop() {
        val running = worker
        val opened = line

        worker = null
        line = null
        opened?.stop()
        opened?.close()

        if (running != null && running !== Thread.currentThread()) {
            running.join()
        }
    }

    private fun listenFor(phrase: String, spotter: KeywordSpotter, opened: TargetDataLine): Boolean {
        val stream = spotter.createStream()
        val chunk = ByteArray(ChunkBytes)
        var heard = false

        while (!heard && opened.isOpen) {
            val read = opened.read(chunk, 0, chunk.size)

            if (read > 0) {
                stream.acceptWaveform(samples(chunk, read), SampleRate)

                while (spotter.isReady(stream)) {
                    spotter.decode(stream)

                    val keyword = spotter.getResult(stream).keyword

                    if (keyword.isNotEmpty()) {
                        spotter.reset(stream)
                        heard = heard || keyword == phrase
                    }
                }
            }
        }

        stream.release()

        return heard
    }
}

private fun samples(chunk: ByteArray, length: Int): FloatArray = FloatArray(length / 2) { index ->
    val sample = ((chunk[index * 2 + 1].toInt() shl 8) or (chunk[index * 2].toInt() and 0xFF)).toShort()

    sample / 32768f
}
