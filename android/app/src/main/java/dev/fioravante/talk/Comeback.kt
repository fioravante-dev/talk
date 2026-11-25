package dev.fioravante.talk

import android.accessibilityservice.AccessibilityService
import android.content.ComponentName
import android.content.Context
import android.os.Build
import android.os.SystemClock
import android.provider.Settings
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityWindowInfo

/**
 * O vigia que traz o talk de volta. Quando a pessoa troca para o teclado dela
 * pelo botão do talk, ele espera esse teclado aparecer e depois fechar, e então
 * põe o talk como teclado de novo: o próximo campo abre na voz.
 *
 * Só olha se há uma janela de teclado na tela; não lê nada do que se digita.
 * A troca usa o SoftKeyboardController, do Android 11 em diante.
 */
class Comeback : AccessibilityService() {
    private lateinit var prefs: Prefs
    private lateinit var self: String
    /** Quando o outro teclado apareceu; 0 = não está na tela. */
    private var otherSince = 0L

    override fun onServiceConnected() {
        prefs = Prefs(this)
        self = ComponentName(this, TalkIme::class.java).flattenToShortString()
    }

    override fun onAccessibilityEvent(e: AccessibilityEvent) {
        if (e.eventType != AccessibilityEvent.TYPE_WINDOWS_CHANGED || Build.VERSION.SDK_INT < 30) return
        if (!::prefs.isInitialized || !prefs.borrowed) {
            otherSince = 0L
            return
        }
        val shown = runCatching { windows.any { it.type == AccessibilityWindowInfo.TYPE_INPUT_METHOD } }.getOrDefault(false)
        val current = Settings.Secure.getString(contentResolver, Settings.Secure.DEFAULT_INPUT_METHOD) ?: ""
        val isSelf = current == self || current == "$packageName/${TalkIme::class.java.name}"
        when {
            // Ainda (ou já) é o talk: a troca não aconteceu ou ele voltou à mão.
            // Quem limpa o aviso é o próprio talk, ao abrir.
            isSelf -> otherSince = 0L
            shown -> if (otherSince == 0L) otherSince = SystemClock.uptimeMillis()
            otherSince != 0L -> {
                // Na troca há um instante sem teclado nenhum; só conta se o outro
                // ficou na tela de verdade.
                val stayed = SystemClock.uptimeMillis() - otherSince > 400
                otherSince = 0L
                if (stayed && softKeyboardController.switchToInputMethod(self)) prefs.borrowed = false
            }
        }
    }

    override fun onInterrupt() {}

    companion object {
        fun enabled(ctx: Context): Boolean {
            val list = Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES) ?: return false
            val me = ComponentName(ctx, Comeback::class.java)
            return list.split(':').any { ComponentName.unflattenFromString(it) == me }
        }
    }
}
