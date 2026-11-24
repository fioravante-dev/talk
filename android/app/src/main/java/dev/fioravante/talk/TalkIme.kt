package dev.fioravante.talk

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.inputmethodservice.InputMethodService
import android.os.Handler
import android.os.Looper
import android.util.TypedValue
import android.view.Gravity
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import kotlin.concurrent.thread

/**
 * O teclado de voz: abre, grava, transcreve (no PC pelo Tailscale ou no próprio
 * celular), digita no campo e volta para o teclado de antes.
 */
class TalkIme : InputMethodService() {
    private enum class State { IDLE, RECORDING, WORKING }

    private val ui = Handler(Looper.getMainLooper())
    private lateinit var prefs: Prefs
    private lateinit var status: TextView
    private lateinit var mic: TextView
    private var state = State.IDLE
    private var recorder: Recorder? = null
    /** null = ainda perguntando; true/false = o PC respondeu ou não. */
    @Volatile private var pcOk: Boolean? = null
    private val unloadLater = Runnable { Local.unload() }

    override fun onCreate() {
        super.onCreate()
        prefs = Prefs(this)
    }

    private fun dp(v: Int) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()

    private fun button(label: String, size: Float = 22f, onClick: () -> Unit) = TextView(this).apply {
        text = label
        textSize = size
        setTextColor(Color.WHITE)
        gravity = Gravity.CENTER
        setPadding(dp(16), dp(10), dp(16), dp(10))
        setOnClickListener { onClick() }
    }

    override fun onCreateInputView(): View {
        status = TextView(this).apply {
            setTextColor(Color.rgb(170, 170, 176))
            textSize = 14f
        }
        val top = LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            addView(status, LinearLayout.LayoutParams(0, -2, 1f))
            addView(button("⚙") { openSettings() })
            addView(button("⌨") { back() })
        }
        mic = TextView(this).apply {
            gravity = Gravity.CENTER
            textSize = 18f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Color.WHITE)
            setOnClickListener { onMic() }
        }
        val center = FrameLayout(this).apply {
            addView(mic, FrameLayout.LayoutParams(dp(112), dp(112), Gravity.CENTER))
        }
        val bottom = LinearLayout(this).apply {
            gravity = Gravity.CENTER
            addView(button("Cancelar", 15f) { cancel() })
            addView(View(this@TalkIme), LinearLayout.LayoutParams(0, 1, 1f))
            addView(button("⌫") { key(KeyEvent.KEYCODE_DEL) })
            addView(button("↵") { enter() })
        }
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.rgb(28, 28, 30))
            setPadding(dp(8), dp(4), dp(8), dp(8))
            addView(top)
            addView(center, LinearLayout.LayoutParams(-1, dp(150)))
            addView(bottom)
        }.also { render() }
    }

    override fun onStartInputView(info: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        render()
        if (state == State.IDLE && prefs.autostart && !restarting) start()
    }

    override fun onFinishInputView(finishingInput: Boolean) {
        super.onFinishInputView(finishingInput)
        // O campo sumiu: o texto não teria onde entrar.
        if (state == State.RECORDING) cancel()
    }

    override fun onDestroy() {
        recorder?.stop()
        Local.unload()
        super.onDestroy()
    }

    private fun onMic() = when (state) {
        State.IDLE -> start()
        State.RECORDING -> finish()
        State.WORKING -> {}
    }

    private fun hasMic() = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

    private fun start() {
        if (!hasMic()) {
            say("Sem permissão do microfone: toque em ⚙")
            return
        }
        val r = Recorder { level -> ui.post { if (state == State.RECORDING) pulse(level) } }
        try {
            r.start()
        } catch (e: Exception) {
            say(e.message ?: "o microfone não abriu")
            return
        }
        recorder = r
        state = State.RECORDING
        pcOk = null
        ui.removeCallbacks(unloadLater)
        render()
        tick()
        val p = prefs
        thread(name = "talk-probe") {
            if (p.mode != "phone") {
                val remote = Remote(p.server, p.token)
                val ok = remote.healthy()
                pcOk = ok
                ui.post { if (state == State.RECORDING) render() }
                if (ok) {
                    remote.warm()
                    runCatching { remote.vocab() }.onSuccess { (v, t) -> p.vocab = v; p.vocabTxt = t }
                    return@thread
                }
            }
            if (p.mode != "pc") Local.preload(p)
        }
    }

    private fun tick() {
        if (state != State.RECORDING) return
        render()
        ui.postDelayed({ tick() }, 500)
    }

    private fun finish() {
        val pcm = recorder?.stop() ?: return
        recorder = null
        if (pcm.size < Recorder.RATE * 3 / 10) {
            state = State.IDLE
            say("curto demais")
            return
        }
        state = State.WORKING
        render()
        val p = prefs
        val usePc = pcOk
        thread(name = "talk-stt") {
            val outcome = runCatching { transcribe(pcm, p, usePc) }
            ui.post {
                state = State.IDLE
                outcome.onSuccess { r ->
                    if (r.where == "celular") ui.postDelayed(unloadLater, 3 * 60_000L)
                    if (r.text.isBlank()) {
                        say("nenhuma fala reconhecida")
                    } else {
                        type(r.text)
                        say("%s · %.1f s".format(r.where, r.seconds))
                        if (p.returnAfter) back()
                    }
                }.onFailure { say("⚠ " + (it.message ?: it.toString())) }
            }
        }
    }

    /** O PC quando dá; se ele falhar no meio, o celular (com modelo baixado). */
    private fun transcribe(pcm: ShortArray, p: Prefs, pcOk: Boolean?): Result = when (p.mode) {
        "pc" -> Remote(p.server, p.token).transcribe(pcm, p.language)
        "phone" -> Local.transcribe(pcm, p)
        else -> if (pcOk != false && p.server.isNotEmpty()) {
            try {
                Remote(p.server, p.token).transcribe(pcm, p.language)
            } catch (e: Exception) {
                if (!p.hasModel) throw e
                Local.transcribe(pcm, p)
            }
        } else {
            Local.transcribe(pcm, p)
        }
    }

    private fun cancel() {
        recorder?.stop()
        recorder = null
        if (state == State.RECORDING) state = State.IDLE
        render()
    }

    /** Com um espaço antes se o texto anterior não terminar em espaço. */
    private fun type(text: String) {
        val ic = currentInputConnection ?: return
        val before = ic.getTextBeforeCursor(1, 0)
        val sep = if (!before.isNullOrEmpty() && !before.last().isWhitespace()) " " else ""
        ic.commitText(sep + text.trim(), 1)
    }

    private fun key(code: Int) {
        val ic = currentInputConnection ?: return
        ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, code))
        ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_UP, code))
    }

    /** A ação do campo (enviar, buscar…) se houver; senão, uma quebra de linha. */
    private fun enter() {
        val action = currentInputEditorInfo?.imeOptions?.and(EditorInfo.IME_MASK_ACTION) ?: EditorInfo.IME_ACTION_NONE
        if (action != EditorInfo.IME_ACTION_NONE && action != EditorInfo.IME_ACTION_UNSPECIFIED) {
            currentInputConnection?.performEditorAction(action)
        } else {
            key(KeyEvent.KEYCODE_ENTER)
        }
    }

    private fun back() {
        if (state == State.RECORDING) cancel()
        if (!switchToPreviousInputMethod()) {
            getSystemService(InputMethodManager::class.java).showInputMethodPicker()
        }
    }

    private fun openSettings() {
        cancel()
        startActivity(Intent(this, SettingsActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    // --- tela -----------------------------------------------------------

    private var message: String? = null

    private fun say(m: String) {
        message = m
        render()
    }

    private fun where(): String = when (prefs.mode) {
        "pc" -> "PC"
        "phone" -> "celular"
        else -> when (pcOk) {
            true -> "PC"
            false -> if (prefs.hasModel) "celular" else "sem PC"
            null -> "procurando o PC…"
        }
    }

    private fun render() {
        if (!::mic.isInitialized) return
        val (label, color) = when (state) {
            State.IDLE -> "Falar" to Color.rgb(58, 58, 60)
            State.RECORDING -> "Parar" to Color.rgb(255, 69, 58)
            State.WORKING -> "…" to Color.rgb(58, 58, 60)
        }
        mic.text = label
        mic.background = GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(color)
        }
        if (state != State.RECORDING) {
            mic.scaleX = 1f
            mic.scaleY = 1f
        }
        status.text = when (state) {
            State.RECORDING -> {
                val s = recorder?.seconds ?: 0
                message = null
                "gravando %d:%02d · %s".format(s / 60, s % 60, where())
            }
            State.WORKING -> "transcrevendo · " + where()
            State.IDLE -> message ?: ("pronto · " + where())
        }
    }

    private fun pulse(level: Float) {
        val s = 1f + (level * 6f).coerceAtMost(0.25f)
        mic.scaleX = s
        mic.scaleY = s
    }
}
