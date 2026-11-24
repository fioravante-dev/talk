package dev.fioravante.talk

import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

/** Um talk de PC, pela rede (Tailscale). Tudo aqui bloqueia: chame fora da thread da tela. */
class Remote(val server: Server, private val token: String) {

    private fun open(path: String, method: String, connectMs: Int, readMs: Int): HttpURLConnection {
        val c = URL(server.url + path).openConnection() as HttpURLConnection
        c.requestMethod = method
        c.connectTimeout = connectMs
        c.readTimeout = readMs
        if (token.isNotEmpty()) c.setRequestProperty("Authorization", "Bearer $token")
        return c
    }

    private fun HttpURLConnection.json(): String {
        val ok = responseCode in 200..299
        val body = (if (ok) inputStream else errorStream)?.bufferedReader()?.readText() ?: "{}"
        disconnect()
        if (!ok) throw RuntimeException(runCatching { JSONObject(body).optString("error") }.getOrNull() ?: "HTTP $responseCode")
        return body
    }

    /** null se o PC respondeu; senão, o motivo. */
    fun check(timeoutMs: Int): String? = try {
        if (JSONObject(open("/health", "GET", timeoutMs, timeoutMs).json()).optBoolean("ok")) null else "resposta estranha"
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
        val j = JSONObject(open("/vocab", "GET", 1500, 5000).json())
        return j.optString("vocab") to j.optString("vocab_txt")
    }

    /** Os talks que este PC achou na tailnet. */
    fun peers(): List<Server> {
        val a = JSONArray(open("/peers", "GET", 1500, 10_000).json())
        return (0 until a.length()).map { a.getJSONObject(it).let { o -> Server(o.getString("name"), o.getString("url")) } }
    }

    fun transcribe(pcm: ShortArray, language: String): Result {
        val c = open("/transcribe?language=$language", "POST", 3000, 120_000)
        c.doOutput = true
        c.setRequestProperty("Content-Type", "audio/wav")
        val wav = wav(pcm)
        c.setFixedLengthStreamingMode(wav.size)
        c.outputStream.use { it.write(wav) }
        return Result.from(JSONObject(c.json()), server.name)
    }

    companion object {
        /**
         * O PC a usar agora: o escolhido nos ajustes, ou — no automático —
         * o primeiro a responder, perguntando a todos ao mesmo tempo.
         */
        fun pick(p: Prefs, timeoutMs: Int = 1500): Remote? {
            val token = p.token
            val chosen = p.servers.filter { p.target == "auto" || it.url == p.target }
            if (chosen.isEmpty()) return null
            val winner = AtomicReference<Remote?>(null)
            val left = AtomicInteger(chosen.size)
            val done = CountDownLatch(1)
            for (s in chosen) thread(name = "talk-pick") {
                val r = Remote(s, token)
                if (r.check(timeoutMs) == null) winner.compareAndSet(null, r)
                if (winner.get() != null || left.decrementAndGet() == 0) done.countDown()
            }
            done.await(timeoutMs + 500L, TimeUnit.MILLISECONDS)
            return winner.get()
        }

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
