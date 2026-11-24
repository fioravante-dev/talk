package dev.fioravante.talk

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import kotlin.math.sqrt

/** O microfone, a 16 kHz mono: o que o Whisper quer, sem conversão. */
class Recorder(private val onLevel: (Float) -> Unit) {
    private var record: AudioRecord? = null
    private var thread: Thread? = null
    @Volatile private var running = false
    private var samples = ShortArray(RATE * 30)
    private var size = 0

    @SuppressLint("MissingPermission") // quem chama já conferiu a permissão
    fun start() {
        val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        // VOICE_RECOGNITION: sem o processamento de chamada (AGC, supressão
        // agressiva) que deixa a voz metálica para o Whisper.
        val r = AudioRecord(
            MediaRecorder.AudioSource.VOICE_RECOGNITION, RATE,
            AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, maxOf(min, RATE),
        )
        if (r.state != AudioRecord.STATE_INITIALIZED) {
            r.release()
            throw IllegalStateException("o microfone não abriu")
        }
        record = r
        size = 0
        running = true
        r.startRecording()
        thread = Thread {
            val buf = ShortArray(RATE / 10)
            while (running) {
                val n = r.read(buf, 0, buf.size)
                if (n <= 0) continue
                synchronized(this) {
                    if (size + n > samples.size) samples = samples.copyOf(samples.size * 2)
                    System.arraycopy(buf, 0, samples, size, n)
                    size += n
                }
                var acc = 0.0
                for (i in 0 until n) acc += buf[i].toDouble() * buf[i]
                onLevel((sqrt(acc / n) / 32768.0).toFloat())
            }
        }.apply { start() }
    }

    /** Para e devolve o que gravou. */
    fun stop(): ShortArray {
        running = false
        thread?.join(500)
        record?.run {
            try { stop() } catch (_: Exception) {}
            release()
        }
        record = null
        return synchronized(this) { samples.copyOf(size) }
    }

    val seconds get() = size / RATE

    companion object {
        const val RATE = 16_000
    }
}
