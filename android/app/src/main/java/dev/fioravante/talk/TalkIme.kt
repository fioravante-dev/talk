package dev.fioravante.talk

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.ColorStateList
import android.graphics.Color
import android.inputmethodservice.InputMethodService
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.TypedValue
import android.view.Gravity
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.WindowInsets
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ProgressBar
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
    private lateinit var look: Palette
    private val mics = mutableListOf<Mic>()
    private lateinit var tools: View
    private lateinit var cancelButton: View
    private lateinit var title: TextView
    private lateinit var caption: TextView
    private lateinit var meter: Meter
    private var builtPos = ""
    private var builtDark = false
    private var state = State.IDLE
    private var recorder: Recorder? = null
    /** null = ainda perguntando; true/false = um PC respondeu ou não. */
    @Volatile private var pcOk: Boolean? = null
    /** O PC que respondeu primeiro nesta ditada. */
    @Volatile private var pc: Remote? = null
    private var probe: Thread? = null
    private val unloadLater = Runnable { Local.unload() }

    override fun onCreate() {
        super.onCreate()
        prefs = Prefs(this)
    }

    private fun dp(v: Int) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()

    /** Um botão de ícone da faixa de cima: redondo, sem fundo, 48 dp. */
    private fun tool(icon: Int, what: String, onClick: () -> Unit) = ImageView(this).apply {
        setImageResource(icon)
        imageTintList = ColorStateList.valueOf(look.onVariant)
        scaleType = ImageView.ScaleType.CENTER
        background = look.surface(Color.TRANSPARENT, 0f, oval = true)
        contentDescription = what
        setOnClickListener { onClick() }
    }

    /** Uma tecla da fileira de baixo, no formato das do Gboard. */
    private fun key(color: Int, radius: Int, onClick: () -> Unit) = FrameLayout(this).apply {
        background = look.surface(color, dp(radius).toFloat())
        setOnClickListener { onClick() }
    }

    private fun icon(res: Int, tint: Int) = ImageView(this).apply {
        setImageResource(res)
        imageTintList = ColorStateList.valueOf(tint)
    }

    private fun centered() = FrameLayout.LayoutParams(-2, -2, Gravity.CENTER)

    /** O botão de falar: um círculo de 48 dp onde fica o microfone do Gboard. */
    private fun newMic() = FrameLayout(this).apply {
        val glyph = ImageView(this@TalkIme)
        val spin = ProgressBar(this@TalkIme).apply {
            isIndeterminate = true
            indeterminateTintList = ColorStateList.valueOf(look.amber)
            visibility = View.GONE
        }
        addView(glyph, centered())
        addView(spin, FrameLayout.LayoutParams(dp(40), dp(40), Gravity.CENTER))
        contentDescription = "Falar"
        setOnClickListener { onMic() }
        mics += Mic(this, glyph, spin)
    }

    private class Mic(val view: FrameLayout, val glyph: ImageView, val spin: ProgressBar)

    /** ⌫ segurado apaga em sequência, como num teclado comum. */
    private fun repeating(view: View, action: () -> Unit) {
        val again = object : Runnable {
            override fun run() {
                action()
                ui.postDelayed(this, 60)
            }
        }
        view.setOnTouchListener { v, e ->
            when (e.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    v.isPressed = true
                    v.drawableHotspotChanged(e.x, e.y)
                    action()
                    ui.postDelayed(again, 400)
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    v.isPressed = false
                    ui.removeCallbacks(again)
                }
            }
            true
        }
    }

    override fun onCreateInputView(): View {
        look = Palette(this)
        builtPos = prefs.micPos
        builtDark = look.dark
        mics.clear()

        // A faixa de cima, na altura da do Gboard: ferramentas e o microfone.
        tools = LinearLayout(this).apply {
            addView(tool(R.drawable.ic_settings, "Ajustes") { openSettings() }, LinearLayout.LayoutParams(dp(48), dp(48)))
            addView(tool(R.drawable.ic_keyboard, "Voltar ao meu teclado") { back() }, LinearLayout.LayoutParams(dp(48), dp(48)))
        }
        cancelButton = tool(R.drawable.ic_close, "Cancelar") { cancel() }
        val bar = FrameLayout(this)
        fun place(v: View, g: Int, w: Int = -2) = bar.addView(v, FrameLayout.LayoutParams(w, dp(48), g or Gravity.CENTER_VERTICAL))
        val micSize = dp(48)
        val (toolsAt, micsAt) = when (builtPos) {
            "left" -> Gravity.END to listOf(Gravity.START)
            "center" -> Gravity.START to listOf(Gravity.CENTER_HORIZONTAL)
            "both" -> Gravity.CENTER_HORIZONTAL to listOf(Gravity.START, Gravity.END)
            else -> Gravity.START to listOf(Gravity.END)
        }
        place(tools, toolsAt)
        place(cancelButton, toolsAt, dp(48))
        for (g in micsAt) place(newMic(), g, micSize)

        // O meio: o que está acontecendo, grande o bastante para ler de relance.
        title = TextView(this).apply {
            setTextColor(look.on)
            textSize = 17f
            gravity = Gravity.CENTER
        }
        meter = Meter(this, look)
        caption = TextView(this).apply {
            setTextColor(look.onVariant)
            textSize = 13f
            gravity = Gravity.CENTER
            maxLines = 2
        }
        val middle = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            addView(title)
            addView(meter, LinearLayout.LayoutParams(dp(240), dp(12)).apply { setMargins(0, dp(12), 0, dp(10)) })
            addView(caption)
            // Gravando, o meio inteiro também para: um alvo grande para o polegar.
            setOnClickListener { if (state == State.RECORDING) finish() }
        }

        // Embaixo: espaço, apagar e enter, como no teclado de sempre.
        val space = key(look.key, 8) { commit(" ") }.apply {
            addView(TextView(this@TalkIme).apply {
                text = "espaço"
                textSize = 14f
                setTextColor(look.onVariant)
            }, centered())
            contentDescription = "Espaço"
        }
        val del = key(look.fn, 8) {}.apply {
            addView(icon(R.drawable.ic_backspace, look.on), centered())
            contentDescription = "Apagar"
        }
        repeating(del) { press(KeyEvent.KEYCODE_DEL) }
        val enter = key(look.fn, 24) { enter() }.apply {
            addView(icon(R.drawable.ic_return, look.on), centered())
            contentDescription = "Enter"
        }
        val keys = LinearLayout(this).apply {
            fun lp(w: Int, weight: Float = 0f) = LinearLayout.LayoutParams(w, -1, weight).apply { setMargins(dp(3), 0, dp(3), 0) }
            addView(space, lp(0, 1f))
            addView(del, lp(dp(64)))
            addView(enter, lp(dp(72)))
        }

        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(look.bg)
            setPadding(dp(6), dp(6), dp(6), dp(10))
            addView(bar, LinearLayout.LayoutParams(-1, dp(48)))
            addView(middle, LinearLayout.LayoutParams(-1, dp(128)))
            addView(keys, LinearLayout.LayoutParams(-1, dp(52)))
            // No Android 15 o teclado vai até a borda: a barra de navegação (e o
            // botão de baixar o teclado) fica por cima do fim dele.
            setOnApplyWindowInsetsListener { v, insets ->
                val nav = if (Build.VERSION.SDK_INT >= 30) {
                    insets.getInsets(WindowInsets.Type.navigationBars()).bottom
                } else {
                    @Suppress("DEPRECATION")
                    insets.systemWindowInsetBottom
                }
                v.setPadding(dp(6), dp(6), dp(6), dp(10) + nav)
                insets
            }
        }.also { render() }
    }

    override fun onStartInputView(info: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        if (prefs.micPos != builtPos || Palette(this).dark != builtDark) setInputView(onCreateInputView())
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
            say("Sem permissão do microfone: abra os ajustes na engrenagem")
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
        pc = null
        ui.removeCallbacks(unloadLater)
        render()
        tick()
        val p = prefs
        probe = thread(name = "talk-probe") {
            if (p.mode != "phone") {
                val remote = Remote.pick(p)
                pc = remote
                pcOk = remote != null
                ui.post { if (state == State.RECORDING) render() }
                if (remote != null) {
                    thread { remote.warm() }
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
        val waitFor = probe
        thread(name = "talk-stt") {
            // Parou antes de algum PC responder: espera a pergunta terminar.
            waitFor?.join(2500)
            val outcome = runCatching { transcribe(pcm, p, pc) }
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
                }.onFailure { say(it.message ?: it.toString()) }
            }
        }
    }

    /** O PC quando dá; se ele falhar no meio, o celular (com modelo baixado). */
    private fun transcribe(pcm: ShortArray, p: Prefs, pc: Remote?): Result = when (p.mode) {
        "pc" -> (pc ?: throw RuntimeException("nenhum PC respondeu")).transcribe(pcm, p.language)
        "phone" -> Local.transcribe(pcm, p)
        else -> if (pc != null) {
            try {
                pc.transcribe(pcm, p.language)
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
    private fun commit(text: String) {
        currentInputConnection?.commitText(text, 1)
    }

    private fun type(text: String) {
        val ic = currentInputConnection ?: return
        val before = ic.getTextBeforeCursor(1, 0)
        val sep = if (!before.isNullOrEmpty() && !before.last().isWhitespace()) " " else ""
        ic.commitText(sep + text.trim(), 1)
    }

    private fun press(code: Int) {
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
            press(KeyEvent.KEYCODE_ENTER)
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
        "pc" -> pc?.server?.name ?: if (pcOk == false) "nenhum PC" else "procurando o PC…"
        "phone" -> "celular"
        else -> when (pcOk) {
            true -> pc?.server?.name ?: "PC"
            false -> if (prefs.hasModel) "celular" else "sem PC"
            null -> "procurando o PC…"
        }
    }

    private fun render() {
        if (mics.isEmpty()) return
        for (m in mics) {
            val (bg, glyph, tint) = when (state) {
                State.IDLE -> Triple(look.fn, R.drawable.ic_mic, look.on)
                State.RECORDING -> Triple(look.red, R.drawable.ic_stop, Color.WHITE)
                State.WORKING -> Triple(look.fn, R.drawable.ic_mic, look.onVariant)
            }
            m.view.background = look.surface(bg, 0f, oval = true)
            m.glyph.setImageResource(glyph)
            m.glyph.imageTintList = ColorStateList.valueOf(tint)
            m.spin.visibility = if (state == State.WORKING) View.VISIBLE else View.GONE
            m.view.contentDescription = if (state == State.RECORDING) "Parar e digitar" else "Falar"
            if (state != State.RECORDING) {
                m.view.scaleX = 1f
                m.view.scaleY = 1f
            }
        }
        val busy = state != State.IDLE
        tools.visibility = if (busy) View.GONE else View.VISIBLE
        cancelButton.visibility = if (busy) View.VISIBLE else View.GONE
        meter.visibility = if (state == State.RECORDING) View.VISIBLE else View.INVISIBLE
        if (state != State.RECORDING) meter.reset()
        when (state) {
            State.RECORDING -> {
                val s = recorder?.seconds ?: 0
                message = null
                title.text = "%d:%02d".format(s / 60, s % 60)
                title.textSize = 28f
                title.fontFeatureSettings = "tnum"
                caption.text = "ouvindo · " + where() + "\ntoque aqui ou no microfone para digitar"
            }
            State.WORKING -> {
                title.text = "Transcrevendo"
                title.textSize = 17f
                title.fontFeatureSettings = null
                caption.text = where()
            }
            State.IDLE -> {
                title.text = message ?: "Toque no microfone para falar"
                title.textSize = 17f
                title.fontFeatureSettings = null
                caption.text = if (message != null) "" else hint()
            }
        }
    }

    /** Em repouso: para onde a próxima ditada vai. */
    private fun hint(): String = when (prefs.mode) {
        "pc" -> "no PC"
        "phone" -> "no celular"
        else -> "no PC pelo Tailscale, ou no celular"
    }

    private fun pulse(level: Float) {
        meter.level(level)
        if (!prefs.micPulse) return
        val s = 1f + (level * 6f).coerceAtMost(0.18f)
        for (m in mics) {
            m.view.scaleX = s
            m.view.scaleY = s
        }
    }
}
