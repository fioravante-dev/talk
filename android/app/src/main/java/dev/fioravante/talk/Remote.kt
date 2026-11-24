package dev.fioravante.talk

import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** O talk do PC, pela rede (Tailscale). Tudo aqui bloqueia: chame fora da thread da tela. */
class Remote(private val server: String, private val token: String) {

    private fun open(path: String, method: String, connectMs: Int, readMs: Int): HttpURLConnection {
        val c = URL(server + path).openConnection() as HttpURLConnection
        c.requestMethod = method
        c.connectTimeout = connectMs
        c.readTimeout = readMs
        if (token.isNotEmpty()) c.setRequestProperty("Authorization", "Bearer $token")
        return c
    }

    private fun HttpURLConnection.json(): JSONObject {
        val ok = responseCode in 200..299
        val body = (if (ok) inputStream else errorStream)?.bufferedReader()?.readText() ?: "{}"
        disconnect()
        val j = JSONObject(body)
        if (!ok) throw RuntimeException(j.optString("error", "HTTP $responseCode"))
        return j
    }

    /** Responde rápido ou não conta: no 4G sem o PC ligado, esperar é pior que cair para o celular. */
    fun healthy(timeoutMs: Int = 1500): Boolean = check(timeoutMs) == null

    /** null se o PC respondeu; senão, o motivo, para mostrar nos ajustes. */
    fun check(timeoutMs: Int): String? = try {
        when {
            server.isEmpty() -> "sem endereço"
            open("/health", "GET", timeoutMs, timeoutMs).json().optBoolean("ok") -> null
            else -> "resposta estranha"
        }
    } catch (e: Exception) {
        "${e.javaClass.simpleName}: ${e.message}"
    }

    /** Começa a carregar o modelo no PC enquanto a pessoa fala. */
    fun warm() {
        try {
            val c = open("/warm", "POST", 1500, 30_000)
            c.doOutput = true
            c.outputStream.close()
            c.json()
        } catch (_: Exception) {
        }
    }

    /** (vocab, vocab_txt) do PC. */
    fun vocab(): Pair<String, String> {
        val j = open("/vocab", "GET", 1500, 5000).json()
        return j.optString("vocab") to j.optString("vocab_txt")
    }

    fun transcribe(pcm: ShortArray, language: String): Result {
        val c = open("/transcribe?language=$language", "POST", 3000, 120_000)
        c.doOutput = true
        c.setRequestProperty("Content-Type", "audio/wav")
        val wav = wav(pcm)
        c.setFixedLengthStreamingMode(wav.size)
        c.outputStream.use { it.write(wav) }
        return Result.from(c.json(), "PC")
    }

    companion object {
        /** PCM de 16 bits, mono, 16 kHz, com o cabeçalho de 44 bytes. */
        fun wav(pcm: ShortArray): ByteArray {
            val data = pcm.size * 2
            val b = ByteBuffer.allocate(44 + data).order(ByteOrder.LITTLE_ENDIAN)
            b.put("RIFF".toByteArray()).putInt(36 + data).put("WAVEfmt ".toByteArray())
            b.putInt(16).putShort(1).putShort(1).putInt(Recorder.RATE).putInt(Recorder.RATE * 2)
            b.putShort(2).putShort(16).put("data".toByteArray()).putInt(data)
            for (s in pcm) b.putShort(s)
            return b.array()
        }
    }
}

data class Result(val text: String, val language: String, val seconds: Double, val where: String) {
    companion object {
        fun from(j: JSONObject, where: String): Result {
            if (j.has("error")) throw RuntimeException(j.getString("error"))
            return Result(j.optString("text"), j.optString("language"), j.optDouble("stt_seconds", 0.0), where)
        }
    }
}
