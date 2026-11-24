package dev.fioravante.talk

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.text.InputType
import android.util.TypedValue
import android.view.View
import android.view.inputmethod.InputMethodManager
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.TextView
import kotlin.concurrent.thread

/** Os ajustes: permissão, ativar o teclado, o PC, o idioma e o modelo do celular. */
class SettingsActivity : Activity() {
    private val ui = Handler(Looper.getMainLooper())
    private lateinit var prefs: Prefs
    private lateinit var root: LinearLayout
    private lateinit var setup: TextView
    private lateinit var server: EditText
    private lateinit var token: EditText
    private lateinit var pcStatus: TextView
    private lateinit var modelStatus: TextView
    private lateinit var downloadButton: Button

    private fun dp(v: Int) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()

    private fun text(s: String, size: Float = 15f, color: Int = Color.rgb(200, 200, 206)) = TextView(this).apply {
        text = s
        textSize = size
        setTextColor(color)
        setPadding(0, dp(6), 0, dp(6))
    }.also { root.addView(it) }

    private fun title(s: String) = text(s, 19f, Color.WHITE).apply { setPadding(0, dp(22), 0, dp(4)) }

    private fun button(s: String, onClick: () -> Unit) = Button(this).apply {
        text = s
        isAllCaps = false
        setOnClickListener { onClick() }
    }.also { root.addView(it) }

    private fun choice(options: List<Pair<String, String>>, current: String, onPick: (String) -> Unit) {
        val group = RadioGroup(this)
        options.forEachIndexed { i, (value, label) ->
            group.addView(RadioButton(this).apply {
                id = View.generateViewId()
                text = label
                isChecked = value == current
                setOnCheckedChangeListener { _, on -> if (on) onPick(options[i].first) }
            })
        }
        root.addView(group)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        prefs = Prefs(this)
        root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(24), dp(20), dp(48))
        }
        setContentView(ScrollView(this).apply {
            fitsSystemWindows = true
            addView(root)
        })

        text("talk", 28f, Color.WHITE)
        text("Ditado por voz: grava aqui, transcreve no PC (pelo Tailscale) ou no próprio celular, e digita no campo.")

        title("1. Ativar")
        setup = text("")
        button("Permitir o microfone") { requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 1) }
        button("Ativar o teclado de voz") { startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) }
        text("Para ditar: toque num campo, troque para \"talk (voz)\" pelo botão de teclado na barra de navegação e fale. Ele volta sozinho para o teu teclado.", 13f)

        title("2. O PC")
        text("O endereço do listen no config.toml do talk no PC.", 13f)
        server = EditText(this).apply {
            hint = "http://100.73.197.50:7878"
            setText(prefs.server)
            inputType = InputType.TYPE_TEXT_VARIATION_URI
        }.also { root.addView(it) }
        token = EditText(this).apply {
            hint = "token (o listen_token, se houver)"
            setText(prefs.token)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
        }.also { root.addView(it) }
        button("Testar o PC") { testPc() }
        pcStatus = text("", 13f)

        title("3. Idioma")
        choice(listOf("pt" to "Português", "en" to "Inglês", "auto" to "Detectar"), prefs.language) { prefs.language = it }

        title("4. Onde transcrever")
        choice(
            listOf(
                "auto" to "No PC quando ele responde; senão, no celular",
                "pc" to "Só no PC",
                "phone" to "Só no celular",
            ),
            prefs.mode,
        ) { prefs.mode = it }
        root.addView(CheckBox(this).apply {
            text = "Começar a gravar ao abrir"
            isChecked = prefs.autostart
            setOnCheckedChangeListener { _, on -> prefs.autostart = on }
        })
        root.addView(CheckBox(this).apply {
            text = "Voltar ao meu teclado depois de digitar"
            isChecked = prefs.returnAfter
            setOnCheckedChangeListener { _, on -> prefs.returnAfter = on }
        })

        title("5. Modelo do celular")
        text("Whisper large-v3-turbo (~550 MB), para ditar sem o PC. Roda na CPU: alguns segundos por frase.", 13f)
        modelStatus = text("", 13f)
        downloadButton = button("Baixar o modelo") { Local.download(this); refreshModel() }

        title("Teste")
        root.addView(EditText(this).apply {
            hint = "Toque aqui e troque para o talk (voz)"
            minLines = 3
        })
    }

    override fun onResume() {
        super.onResume()
        refreshSetup()
        refreshModel()
    }

    override fun onPause() {
        super.onPause()
        prefs.server = server.text.toString()
        prefs.token = token.text.toString()
        ui.removeCallbacksAndMessages(null)
    }

    override fun onRequestPermissionsResult(code: Int, permissions: Array<out String>, results: IntArray) {
        refreshSetup()
    }

    private fun refreshSetup() {
        val mic = checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        val enabled = getSystemService(InputMethodManager::class.java).enabledInputMethodList.any { it.packageName == packageName }
        setup.text = "Microfone: ${if (mic) "✓ permitido" else "✗ falta permitir"}\n" +
            "Teclado de voz: ${if (enabled) "✓ ativado" else "✗ falta ativar"}"
    }

    private fun testPc() {
        prefs.server = server.text.toString()
        prefs.token = token.text.toString()
        pcStatus.text = "perguntando…"
        val p = prefs
        thread {
            val remote = Remote(p.server, p.token)
            val msg = if (remote.healthy(3000)) {
                runCatching { remote.vocab() }.fold(
                    { (v, t) -> p.vocab = v; p.vocabTxt = t; "✓ o PC respondeu; vocabulário trazido" },
                    { "✓ o PC respondeu, mas o vocabulário não veio: ${it.message}" },
                )
            } else {
                "✗ sem resposta. O Tailscale está ligado aqui? O talk do PC tem listen no config?"
            }
            ui.post { pcStatus.text = msg }
        }
    }

    private fun refreshModel() {
        val progress = Local.progress(this)
        when {
            progress != null -> {
                val (done, total) = progress
                modelStatus.text = if (total > 0) "baixando: %d de %d MB".format(done shr 20, total shr 20) else "baixando…"
                downloadButton.isEnabled = false
                ui.postDelayed({ refreshModel() }, 1000)
            }
            prefs.hasModel -> {
                modelStatus.text = "✓ baixado (%d MB)".format(prefs.modelFile.length() shr 20)
                downloadButton.isEnabled = true
                downloadButton.text = "Baixar de novo"
            }
            else -> {
                modelStatus.text = "não baixado"
                downloadButton.isEnabled = true
            }
        }
    }
}
