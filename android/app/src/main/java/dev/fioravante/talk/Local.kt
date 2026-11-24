package dev.fioravante.talk

import android.app.DownloadManager
import android.content.Context
import android.net.Uri
import org.json.JSONObject
import java.io.File
import java.util.concurrent.ExecutionException
import java.util.concurrent.Executors

/** O modelo no celular: carregar, transcrever e baixar. Uma thread só, na ordem. */
object Local {
    private val exec = Executors.newSingleThreadExecutor()
    @Volatile private var loadedPath: String? = null

    private fun load(p: Prefs) {
        val path = p.modelFile.path
        if (loadedPath != path) {
            Engine.load(path)
            loadedPath = path
        }
    }

    /** Começa a carregar já (alguns segundos), enquanto a pessoa fala. */
    fun preload(p: Prefs) {
        if (p.hasModel) exec.execute { runCatching { load(p) } }
    }

    fun transcribe(pcm: ShortArray, p: Prefs): Result {
        if (!p.hasModel) throw RuntimeException("sem o modelo do celular (baixe nos ajustes)")
        val task = exec.submit<Result> {
            load(p)
            val audio = FloatArray(pcm.size) { pcm[it] / 32768f }
            val vad = if (p.vadFile.exists()) p.vadFile.path else ""
            Result.from(JSONObject(Engine.transcribe(audio, p.language, p.vocabTxt, p.vocab, vad)), "celular")
        }
        try {
            return task.get()
        } catch (e: ExecutionException) {
            throw e.cause ?: e
        }
    }

    /** Devolve a RAM (~1 GB com o modelo grande). */
    fun unload() = exec.execute {
        if (loadedPath != null) {
            Engine.unload()
            loadedPath = null
        }
    }

    // --- download -------------------------------------------------------

    /** Baixa o modelo e o VAD pelo DownloadManager (segue com o app fechado). */
    fun download(context: Context) {
        val dm = context.getSystemService(DownloadManager::class.java)
        val p = Prefs(context)
        p.modelFile.parentFile?.mkdirs()
        val ids = listOf(Prefs.MODEL_URL to Prefs.MODEL, Prefs.VAD_URL to Prefs.VAD).map { (url, name) ->
            File(p.modelFile.parentFile, "$name.part").delete()
            dm.enqueue(
                DownloadManager.Request(Uri.parse(url))
                    .setTitle("talk: $name")
                    .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE)
                    .setDestinationInExternalFilesDir(context, null, "models/$name.part"),
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
        val dir = Prefs(context).modelFile.parentFile
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
                            if (f.exists()) f.renameTo(File(dir, f.name.removeSuffix(".part")))
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
