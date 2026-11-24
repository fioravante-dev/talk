package dev.fioravante.talk

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/** Um talk de PC na tailnet. */
data class Server(val name: String, val url: String)

/** Um modelo para transcrever no próprio celular, com os arquivos que ele precisa. */
data class LocalModel(
    val id: String,
    val label: String,
    /** Subpasta em models/ ("" = a própria). */
    val dir: String,
    /** (url, nome do arquivo); o primeiro é o principal. */
    val files: List<Pair<String, String>>,
    /** Abaixo disto o arquivo principal está pela metade. */
    val minBytes: Long,
)

/** Os ajustes do app, e onde ficam os modelos baixados. */
class Prefs(context: Context) {
    private val p = context.getSharedPreferences("talk", Context.MODE_PRIVATE)
    val models = File(context.getExternalFilesDir(null), "models")

    /** Os PCs conhecidos, na ordem da lista. Começa com os da tailnet do Pedro. */
    var servers: List<Server>
        get() {
            val raw = p.getString("servers", null) ?: return defaults()
            val a = JSONArray(raw)
            return (0 until a.length()).map { a.getJSONObject(it).let { o -> Server(o.getString("name"), o.getString("url")) } }
        }
        set(v) {
            val a = JSONArray()
            v.forEach { a.put(JSONObject().put("name", it.name).put("url", it.url)) }
            p.edit().putString("servers", a.toString()).apply()
        }

    private fun defaults(): List<Server> {
        val list = mutableListOf(
            Server("pop-os", "http://100.73.197.50:7878"),
            Server("pedroc", "http://100.100.101.82:7878"),
        )
        // O endereço único das versões anteriores entra na lista.
        val old = normalize(p.getString("server", "") ?: "")
        if (old.isNotEmpty() && list.none { it.url == old }) list.add(0, Server(old.substringAfter("://").substringBefore(":"), old))
        return list
    }

    /** "auto" (o que responder primeiro) ou a url de um PC. */
    var target: String
        get() = p.getString("target", "auto") ?: "auto"
        set(v) = p.edit().putString("target", v).apply()
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
    /** Começa a gravar assim que o teclado de voz abre. Desligado por padrão:
     *  abrir o teclado sem querer gravava. A chave nova zera quem tinha o antigo. */
    var autostart: Boolean
        get() = p.getBoolean("autostart_v2", false)
        set(v) = p.edit().putBoolean("autostart_v2", v).apply()
    /** Onde fica o botão de falar: center, left, right ou both. */
    var micSide: String
        get() = p.getString("mic_side", "center") ?: "center"
        set(v) = p.edit().putString("mic_side", v).apply()
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

    var localModelId: String
        get() = p.getString("local_model", PARAKEET.id) ?: PARAKEET.id
        set(v) = p.edit().putString("local_model", v).apply()
    val localModel get() = MODELS.firstOrNull { it.id == localModelId } ?: PARAKEET

    fun dir(m: LocalModel) = if (m.dir.isEmpty()) models else File(models, m.dir)
    fun file(m: LocalModel, name: String) = File(dir(m), name)
    fun has(m: LocalModel) =
        m.files.all { file(m, it.second).exists() } && file(m, m.files[0].second).length() >= m.minBytes
    val hasModel get() = has(localModel)

    companion object {
        /** "100.73.197.50" ou "pop-os:7878" viram "http://100.73.197.50:7878". */
        fun normalize(v: String): String {
            var s = v.trim().trimEnd('/')
            if (s.isEmpty()) return s
            if (!s.startsWith("http://") && !s.startsWith("https://")) s = "http://$s"
            if (!Regex(":\\d+$").containsMatchIn(s.substringAfter("://"))) s += ":7878"
            return s
        }

        private const val HF_PARAKEET = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/main"
        val PARAKEET = LocalModel(
            "parakeet", "Parakeet v3 (NVIDIA): rápido, com pontuação · ~670 MB", "parakeet-v3",
            listOf("encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt").map { "$HF_PARAKEET/$it" to it },
            600_000_000,
        )
        const val VAD = "ggml-silero-v5.1.2.bin"
        val WHISPER = LocalModel(
            "whisper", "Whisper large-v3-turbo: mais lento, aceita o vocabulário · ~550 MB", "",
            listOf(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin" to "ggml-large-v3-turbo-q5_0.bin",
                "https://huggingface.co/ggml-org/whisper-vad/resolve/main/$VAD" to VAD,
            ),
            100_000_000,
        )
        val MODELS = listOf(PARAKEET, WHISPER)
    }
}
