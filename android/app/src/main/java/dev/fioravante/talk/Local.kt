package dev.fioravante.talk

import android.app.DownloadManager
import android.content.Context
import android.net.Uri
import com.k2fsa.sherpa.onnx.FeatureConfig
import com.k2fsa.sherpa.onnx.OfflineModelConfig
import com.k2fsa.sherpa.onnx.OfflineRecognizer
import com.k2fsa.sherpa.onnx.OfflineRecognizerConfig
import com.k2fsa.sherpa.onnx.OfflineTransducerModelConfig
import org.json.JSONObject
import java.io.File
import java.util.concurrent.ExecutionException
import java.util.concurrent.Executors

/** O modelo no celular: carregar, transcrever e baixar. Uma thread só, na ordem. */
object Local {
    private val exec = Executors.newSingleThreadExecutor()
    @Volatile private var loaded: String? = null
    private var parakeet: OfflineRecognizer? = null

    private fun load(p: Prefs) {
        val m = p.localModel
        if (loaded == m.id) return
        drop()
        when (m.id) {
            Prefs.PARAKEET.id -> {
                val f = { name: String -> p.file(m, name).path }
                parakeet = OfflineRecognizer(
                    config = OfflineRecognizerConfig(
                        featConfig = FeatureConfig(sampleRate = Recorder.RATE, featureDim = 80),
                        modelConfig = OfflineModelConfig(
                            transducer = OfflineTransducerModelConfig(
                                encoder = f("encoder.int8.onnx"),
                                decoder = f("decoder.int8.onnx"),
                                joiner = f("joiner.int8.onnx"),
                            ),
                            tokens = f("tokens.txt"),
                            modelType = "nemo_transducer",
                            // Os 4 núcleos grandes do Snapdragon; os pequenos só atrasariam.
                            numThreads = 4,
                        ),
                        decodingMethod = "greedy_search",
                    ),
                )
            }
            else -> Engine.load(p.file(m, m.files[0].second).path)
        }
        loaded = m.id
    }

    private fun drop() {
        parakeet?.release()
        parakeet = null
        if (loaded == Prefs.WHISPER.id) Engine.unload()
        loaded = null
    }

    /** Começa a carregar já, enquanto a pessoa fala. */
    fun preload(p: Prefs) {
        if (p.hasModel) exec.execute { runCatching { load(p) } }
    }

    fun transcribe(pcm: ShortArray, p: Prefs): Result {
        if (!p.hasModel) throw RuntimeException("sem o modelo do celular (baixe nos ajustes)")
        val task = exec.submit<Result> {
            load(p)
            val audio = FloatArray(pcm.size) { pcm[it] / 32768f }
            val rec = parakeet
            if (rec != null) {
                val t = System.nanoTime()
                val stream = rec.createStream()
                stream.acceptWaveform(audio, Recorder.RATE)
                rec.decode(stream)
                val text = rec.getResult(stream).text.trim()
                stream.release()
                Result(Engine.applyVocab(text, p.vocabTxt, p.vocab), "", (System.nanoTime() - t) / 1e9, "celular")
            } else {
                val vad = p.file(Prefs.WHISPER, Prefs.VAD).let { if (it.exists()) it.path else "" }
                Result.from(JSONObject(Engine.transcribe(audio, p.language, p.vocabTxt, p.vocab, vad)), "celular")
            }
        }
        try {
            return task.get()
        } catch (e: ExecutionException) {
            throw e.cause ?: e
        }
    }

    /** Devolve a RAM (o modelo ocupa ~1 GB). */
    fun unload() = exec.execute { drop() }

    // --- download -------------------------------------------------------

    /** Baixa o que falta do modelo pelo DownloadManager (segue com o app fechado). */
    fun download(context: Context, m: LocalModel) {
        val dm = context.getSystemService(DownloadManager::class.java)
        val p = Prefs(context)
        p.dir(m).mkdirs()
        val sub = if (m.dir.isEmpty()) "models" else "models/${m.dir}"
        val ids = m.files.filter { (_, name) -> !p.file(m, name).exists() }.map { (url, name) ->
            File(p.dir(m), "$name.part").delete()
            dm.enqueue(
                DownloadManager.Request(Uri.parse(url))
                    .setTitle("talk: $name")
                    .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE)
                    .setDestinationInExternalFilesDir(context, null, "$sub/$name.part"),
            )
        }
        context.getSharedPreferences("talk", Context.MODE_PRIVATE).edit()
            .putString("downloads", ids.joinToString(",")).apply()
    }

    /**
     * Onde o download está: null se nenhum, senão (baixados, total) em bytes.
     * O que terminou troca o `.part` pelo nome final: um modelo pela metade
     * nunca é carregado.
     */
    fun progress(context: Context): Pair<Long, Long>? {
        val sp = context.getSharedPreferences("talk", Context.MODE_PRIVATE)
        val ids = sp.getString("downloads", "")!!.split(",").mapNotNull { it.toLongOrNull() }
        if (ids.isEmpty()) return null
        val dm = context.getSystemService(DownloadManager::class.java)
        var done = 0L
        var total = 0L
        var pending = false
        dm.query(DownloadManager.Query().setFilterById(*ids.toLongArray())).use { c ->
            while (c.moveToNext()) {
                val status = c.getInt(c.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS))
                done += c.getLong(c.getColumnIndexOrThrow(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR))
                total += maxOf(0, c.getLong(c.getColumnIndexOrThrow(DownloadManager.COLUMN_TOTAL_SIZE_BYTES)))
                when (status) {
                    DownloadManager.STATUS_SUCCESSFUL -> {
                        val part = Uri.parse(c.getString(c.getColumnIndexOrThrow(DownloadManager.COLUMN_LOCAL_URI))).path
                        if (part != null) {
                            val f = File(part)
                            if (f.exists()) f.renameTo(File(f.parentFile, f.name.removeSuffix(".part")))
                        }
                    }
                    DownloadManager.STATUS_FAILED -> {}
                    else -> pending = true
                }
            }
        }
        if (!pending) sp.edit().remove("downloads").apply()
        return if (pending) done to total else null
    }
}
