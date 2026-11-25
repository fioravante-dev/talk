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
import android.widget.Toast
import kotlin.concurrent.thread

/** Os ajustes: permissão, ativar o teclado, os PCs, o idioma e o modelo do celular. */
class SettingsActivity : Activity() {
    private val ui = Handler(Looper.getMainLooper())
    private lateinit var prefs: Prefs
    private lateinit var root: LinearLayout
    private lateinit var setup: TextView
    private lateinit var comeback: TextView
    private lateinit var serverGroup: RadioGroup
    private lateinit var token: EditText
    private lateinit var pcStatus: TextView
    private lateinit var modelStatus: TextView
    private lateinit var downloadButton: Button
    /** O que cada PC respondeu na última sondagem: url → "✓ 40 ms" ou "✗ …". */
    private val reach = mutableMapOf<String, String>()

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
        options.forEach { (value, label) ->
            group.addView(RadioButton(this).apply {
                id = View.generateViewId()
                text = label
                isChecked = value == current
                setOnCheckedChangeListener { _, on -> if (on) onPick(value) }
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
        text("Ditado por voz: grava aqui, transcreve num PC da tailnet (ou no próprio celular) e digita no campo.")

        title("1. Ativar")
        setup = text("")
        button("Permitir o microfone") { requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 1) }
        button("Ativar o teclado de voz") { startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) }
        text("Para ditar: toque num campo, troque para \"talk (voz)\" pelo botão de teclado na barra de navegação e fale. Ele volta sozinho para o teu teclado.", 13f)
        comeback = text("")
        button("Voltar sempre ao talk") { startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)) }
        text("Com isto ligado (Acessibilidade → talk), quando você troca para o teu teclado pelo talk, o talk volta a ser o teclado assim que o outro fechar: o próximo campo abre na voz. " +
            "Se o Android disser que a opção está restrita: Ajustes → Apps → talk → ⋮ → Permitir configurações restritas.", 13f)

        title("2. Os PCs")
        text("Precisa do Tailscale ligado aqui. No automático, a cada ditada vale o primeiro PC que responder. Toque e segure um PC para tirá-lo da lista.", 13f)
        serverGroup = RadioGroup(this).also { root.addView(it) }
        pcStatus = text("", 13f)
        button("Procurar talks no Tailscale") { discover() }
        val add = EditText(this).apply {
            hint = "outro endereço: 100.x.y.z ou nome:porta"
            inputType = InputType.TYPE_TEXT_VARIATION_URI
        }.also { root.addView(it) }
        button("Adicionar") {
            val url = Prefs.normalize(add.text.toString())
            if (url.isNotEmpty() && prefs.servers.none { it.url == url }) {
                prefs.servers = prefs.servers + Server(url.substringAfter("://").substringBefore(":"), url)
                add.setText("")
                renderServers()
                probeAll()
            }
        }
        token = EditText(this).apply {
            hint = "token (o listen_token do PC, se houver)"
            setText(prefs.token)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
        }.also { root.addView(it) }

        title("3. Idioma")
        choice(listOf("pt" to "Português", "en" to "Inglês", "auto" to "Detectar"), prefs.language) { prefs.language = it }
        text("O Parakeet do celular detecta o idioma sozinho; isto vale para o PC e o Whisper.", 13f)

        title("4. Onde transcrever")
        choice(
            listOf(
                "auto" to "No PC quando algum responde; senão, no celular",
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
        text("Para ditar sem PC. Baixa uma vez, pelo Wi-Fi de preferência.", 13f)
        choice(Prefs.MODELS.map { it.id to it.label }, prefs.localModelId) {
            prefs.localModelId = it
            Local.unload()
            refreshModel()
        }
        modelStatus = text("", 13f)
        downloadButton = button("Baixar o modelo") { Local.download(this, prefs.localModel); refreshModel() }

        title("Teste")
        root.addView(EditText(this).apply {
            hint = "Toque aqui e troque para o talk (voz)"
            minLines = 3
        })
    }

    override fun onResume() {
        super.onResume()
        refreshSetup()
        renderServers()
        probeAll()
        refreshModel()
    }

    override fun onPause() {
        super.onPause()
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
        comeback.text = when {
            android.os.Build.VERSION.SDK_INT < 30 -> "Voltar ao talk: precisa do Android 11 ou mais novo"
            Comeback.enabled(this) -> "Voltar ao talk: ✓ ligado"
            else -> "Voltar ao talk: desligado"
        }
    }

    // --- PCs ------------------------------------------------------------

    private fun renderServers() {
        serverGroup.setOnCheckedChangeListener(null)
        serverGroup.removeAllViews()
        val options = listOf("auto" to "Automático: o primeiro que responder") +
            prefs.servers.map { it.url to "${it.name} · ${it.url.substringAfter("://")}  ${reach[it.url] ?: ""}" }
        for ((value, label) in options) {
            serverGroup.addView(RadioButton(this).apply {
                id = View.generateViewId()
                text = label
                tag = value
                isChecked = value == prefs.target
                if (value != "auto") setOnLongClickListener { remove(value); true }
            })
        }
        serverGroup.setOnCheckedChangeListener { g, id -> prefs.target = g.findViewById<View>(id).tag as String }
    }

    private fun remove(url: String) {
        prefs.servers = prefs.servers.filter { it.url != url }
        if (prefs.target == url) prefs.target = "auto"
        Toast.makeText(this, "removido", Toast.LENGTH_SHORT).show()
        renderServers()
    }

    /** Pergunta a cada PC da lista, ao mesmo tempo, e mostra quem respondeu. */
    private fun probeAll() {
        prefs.token = token.text.toString()
        val p = prefs
        pcStatus.text = "perguntando aos PCs…"
        thread {
            val results = p.servers.map { s ->
                s to thread {
                    val t = System.nanoTime()
                    val problem = Remote(s, p.token).check(3000)
                    val ms = (System.nanoTime() - t) / 1_000_000
                    synchronized(reach) { reach[s.url] = if (problem == null) "✓ $ms ms" else "✗" }
                    if (problem != null) synchronized(reach) { reach["!" + s.url] = problem }
                }
            }
            results.forEach { it.second.join() }
            val up = p.servers.filter { reach[it.url]?.startsWith("✓") == true }
            // O vocabulário vem do primeiro que respondeu.
            up.firstOrNull()?.let { s ->
                runCatching { Remote(s, p.token).vocab() }.onSuccess { (v, t) -> p.vocab = v; p.vocabTxt = t }
            }
            ui.post {
                renderServers()
                pcStatus.text = if (up.isNotEmpty()) {
                    "✓ ${up.size} PC(s) respondendo; vocabulário trazido do ${up.first().name}"
                } else {
                    "✗ nenhum PC respondeu. " + (p.servers.firstOrNull()?.let { reach["!" + it.url] } ?: "")
                }
            }
        }
    }

    /** Pede a um PC que responde a lista de talks da tailnet. */
    private fun discover() {
        prefs.token = token.text.toString()
        val p = prefs
        pcStatus.text = "procurando…"
        thread {
            val found = p.servers.firstNotNullOfOrNull { s -> runCatching { Remote(s, p.token).peers() }.getOrNull() }
            ui.post {
                if (found == null) {
                    pcStatus.text = "✗ nenhum PC da lista respondeu para procurar os outros"
                    return@post
                }
                val fresh = found.filter { f -> p.servers.none { it.url == f.url } }
                // Os achados ganham o nome que o Tailscale dá à máquina.
                p.servers = p.servers.map { s -> found.firstOrNull { it.url == s.url } ?: s } + fresh
                pcStatus.text = "✓ ${found.size} talk(s) na tailnet" + if (fresh.isNotEmpty()) ", ${fresh.size} novo(s)" else ""
                renderServers()
                probeAll()
            }
        }
    }

    // --- modelo ---------------------------------------------------------

    private fun refreshModel() {
        val progress = Local.progress(this)
        val m = prefs.localModel
        when {
            progress != null -> {
                val (done, total) = progress
                modelStatus.text = if (total > 0) "baixando: %d de %d MB".format(done shr 20, total shr 20) else "baixando…"
                downloadButton.isEnabled = false
                ui.postDelayed({ refreshModel() }, 1000)
            }
            prefs.has(m) -> {
                modelStatus.text = "✓ baixado"
                downloadButton.isEnabled = false
                downloadButton.text = "Baixado"
            }
            else -> {
                modelStatus.text = "não baixado"
                downloadButton.isEnabled = true
                downloadButton.text = "Baixar o modelo"
            }
        }
    }
}
