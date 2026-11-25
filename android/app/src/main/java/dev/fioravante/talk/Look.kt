package dev.fioravante.talk

import android.content.Context
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.os.Build

/**
 * As cores do teclado: as do sistema (Material You, tiradas do papel de parede,
 * como o Gboard) no Android 12+, e um cinza do Gboard antes disso. Claro ou
 * escuro segue o tema do celular. As três luzes são as do talk no PC: verde
 * pronto, vermelho gravando, âmbar transcrevendo.
 */
class Palette(private val ctx: Context) {
    val dark = (ctx.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
    private val dynamic = Build.VERSION.SDK_INT >= 31
    private fun sys(id: Int, fallback: Int) = if (dynamic) ctx.getColor(id) else fallback

    val bg = if (dark) sys(android.R.color.system_neutral1_900, 0xFF1F1F1F.toInt()) else sys(android.R.color.system_neutral1_50, 0xFFEEF0F3.toInt())
    val key = if (dark) sys(android.R.color.system_neutral1_800, 0xFF3A3B3E.toInt()) else sys(android.R.color.system_neutral1_10, 0xFFFFFFFF.toInt())
    val fn = if (dark) sys(android.R.color.system_accent2_700, 0xFF3A4556.toInt()) else sys(android.R.color.system_accent2_100, 0xFFD3DCEA.toInt())
    val on = if (dark) sys(android.R.color.system_neutral1_50, 0xFFE8EAED.toInt()) else sys(android.R.color.system_neutral1_900, 0xFF1F1F1F.toInt())
    val onVariant = if (dark) sys(android.R.color.system_neutral2_200, 0xFFBDC1C6.toInt()) else sys(android.R.color.system_neutral2_700, 0xFF5F6368.toInt())
    val ripple = if (dark) 0x33FFFFFF else 0x22000000

    val green = if (dark) 0xFF5DB36A.toInt() else 0xFF3F8F4A.toInt()
    val red = if (dark) 0xFFE5533D.toInt() else 0xFFD8442E.toInt()
    val amber = if (dark) 0xFFE0A030.toInt() else 0xFFC98A1B.toInt()

    /** Um fundo com o toque do Material (a onda), na forma pedida. */
    fun surface(color: Int, radius: Float, oval: Boolean = false): Drawable {
        fun shape(c: Int) = GradientDrawable().apply {
            if (oval) shape = GradientDrawable.OVAL else cornerRadius = radius
            setColor(c)
        }
        val content = if (color == Color.TRANSPARENT) null else shape(color)
        return RippleDrawable(ColorStateList.valueOf(ripple), content, shape(Color.BLACK))
    }

}
