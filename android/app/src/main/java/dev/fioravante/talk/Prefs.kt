package dev.fioravante.talk

import android.content.Context
import java.io.File

/** Os ajustes do app, e onde ficam os modelos baixados. */
class Prefs(context: Context) {
    private val p = context.getSharedPreferences("talk", Context.MODE_PRIVATE)
    private val models = File(context.getExternalFilesDir(null), "models")

    /** Ex.: http://100.73.197.50:7878 (o `listen` do config.toml do PC). */
    var server: String
        get() = p.getString("server", "") ?: ""
        set(v) = p.edit().putString("server", normalize(v)).apply()
    var token: String
        get() = p.getString("token", "") ?: ""
        set(v) = p.edit().putString("token", v.trim()).apply()
    /** pt, en ou auto. */
    var language: String
        get() = p.getString("language", "pt") ?: "pt"
        set(v) = p.edit().putString("language", v).apply()
    /** auto (PC quando dá, senão celular), pc, phone. */
    var mode: String
        get() = p.getString("mode", "auto") ?: "auto"
        set(v) = p.edit().putString("mode", v).apply()
    /** Começa a gravar assim que o teclado de voz abre. */
    var autostart: Boolean
        get() = p.getBoolean("autostart", true)
        set(v) = p.edit().putBoolean("autostart", v).apply()
    /** Depois de digitar, volta para o teclado de antes. */
    var returnAfter: Boolean
        get() = p.getBoolean("return_after", true)
        set(v) = p.edit().putBoolean("return_after", v).apply()

    /** O vocabulário do PC, trazido na última vez que ele respondeu. */
    var vocab: String
        get() = p.getString("vocab", "") ?: ""
        set(v) = p.edit().putString("vocab", v).apply()
    var vocabTxt: String
        get() = p.getString("vocab_txt", "") ?: ""
        set(v) = p.edit().putString("vocab_txt", v).apply()

    val modelFile get() = File(models, MODEL)
    val vadFile get() = File(models, VAD)
    val hasModel get() = modelFile.length() > 100_000_000

    companion object {
        /** "100.73.197.50" ou "pop-os:7878" viram "http://100.73.197.50:7878". */
        fun normalize(v: String): String {
            var s = v.trim().trimEnd('/')
            if (s.isEmpty()) return s
            if (!s.startsWith("http://") && !s.startsWith("https://")) s = "http://$s"
            if (!Regex(":\\d+$").containsMatchIn(s.substringAfter("://"))) s += ":7878"
            return s
        }

        const val MODEL = "ggml-large-v3-turbo-q5_0.bin"
        const val MODEL_URL = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$MODEL"
        const val VAD = "ggml-silero-v5.1.2.bin"
        const val VAD_URL = "https://huggingface.co/ggml-org/whisper-vad/resolve/main/$VAD"
    }
}
