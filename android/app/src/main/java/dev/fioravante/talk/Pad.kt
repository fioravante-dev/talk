package dev.fioravante.talk

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ColorFilter
import android.graphics.Outline
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.PorterDuff
import android.graphics.PorterDuffColorFilter
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.drawable.Drawable
import android.graphics.drawable.RippleDrawable
import android.os.SystemClock
import android.view.View
import kotlin.math.PI
import kotlin.math.log10
import kotlin.math.sin

/**
 * A forma da área de falar: um retângulo arredondado com um recorte no canto de
 * cima à esquerda, onde ficam a engrenagem e o teclado. As quinas do recorte são
 * arredondadas para fora, como se a área contornasse os botões.
 */
class NotchShape(
    private val color: Int,
    private val radius: Float,
    private val notchW: Float,
    private val notchH: Float,
) : Drawable() {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { this.color = this@NotchShape.color }
    val path = Path()

    override fun onBoundsChange(b: Rect) {
        val l = b.left.toFloat()
        val t = b.top.toFloat()
        val r = b.right.toFloat()
        val btm = b.bottom.toFloat()
        val k = radius
        val nx = l + notchW
        val ny = t + notchH
        path.reset()
        // Começa embaixo do recorte, na borda esquerda, e segue no sentido horário.
        path.moveTo(l, btm - k)
        path.lineTo(l, ny + k)
        path.quadTo(l, ny, l + k, ny)
        path.lineTo(nx - k, ny)
        path.quadTo(nx, ny, nx, ny - k)
        path.lineTo(nx, t + k)
        path.quadTo(nx, t, nx + k, t)
        path.lineTo(r - k, t)
        path.quadTo(r, t, r, t + k)
        path.lineTo(r, btm - k)
        path.quadTo(r, btm, r - k, btm)
        path.lineTo(l + k, btm)
        path.quadTo(l, btm, l, btm - k)
        path.close()
    }

    override fun draw(canvas: Canvas) = canvas.drawPath(path, paint)
    override fun setAlpha(alpha: Int) { paint.alpha = alpha }
    override fun setColorFilter(cf: ColorFilter?) { paint.colorFilter = cf }
    @Deprecated("Deprecated in Java")
    override fun getOpacity() = PixelFormat.TRANSLUCENT
    override fun getOutline(outline: Outline) {
        @Suppress("DEPRECATION")
        outline.setConvexPath(path)
    }
}

/**
 * O fundo da área com a onda do Material, recortada na mesma forma.
 */
fun padSurface(look: Palette, color: Int, radius: Float, notchW: Float, notchH: Float): Drawable =
    RippleDrawable(
        ColorStateList.valueOf(look.ripple),
        NotchShape(color, radius, notchW, notchH),
        NotchShape(Color.BLACK, radius, notchW, notchH),
    )

/**
 * O círculo do microfone, no alto da área. Em repouso respira devagar; gravando,
 * fica vermelho e as ondas em volta crescem com a voz; transcrevendo, um arco
 * âmbar dá a volta nele.
 */
class Orb(ctx: Context, private val look: Palette) : View(ctx) {
    enum class Mode { IDLE, RECORDING, WORKING }

    var mode = Mode.IDLE
        set(v) {
            field = v
            invalidate()
        }
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private val arc = RectF()
    private val mic = ctx.getDrawable(R.drawable.ic_mic)!!.mutate()
    private val stop = ctx.getDrawable(R.drawable.ic_stop)!!.mutate()
    private var level = 0f
    private val born = SystemClock.uptimeMillis()

    /** RMS de 0 a 1, como o do medidor. */
    fun level(rms: Float) {
        val target = loudness(rms)
        level = if (target > level) target else level + (target - level) * 0.3f
    }

    override fun onDraw(canvas: Canvas) {
        val cx = width / 2f
        val cy = height / 2f
        val r = height * 0.30f
        val t = (SystemClock.uptimeMillis() - born) / 1000f
        when (mode) {
            Mode.IDLE -> {
                // Um halo que respira a cada 3 s, discreto.
                val b = (sin(t * 2 * PI / 3) * 0.5 + 0.5).toFloat()
                fill.color = withAlpha(look.fn, (60 + 50 * b).toInt())
                canvas.drawCircle(cx, cy, r * (1.18f + 0.10f * b), fill)
                fill.color = look.fn
                canvas.drawCircle(cx, cy, r, fill)
                glyph(canvas, mic, look.on, cx, cy, r)
            }
            Mode.RECORDING -> {
                // Três ondas saindo do círculo; o tamanho delas é a voz.
                ring.strokeWidth = r * 0.10f
                for (i in 0 until 3) {
                    val phase = ((t * 0.9f + i / 3f) % 1f)
                    val grow = 1f + phase * (0.35f + level * 0.9f)
                    ring.color = withAlpha(look.red, ((1f - phase) * 150).toInt())
                    canvas.drawCircle(cx, cy, r * grow, ring)
                }
                fill.color = withAlpha(look.red, 70)
                canvas.drawCircle(cx, cy, r * (1.05f + level * 0.35f), fill)
                fill.color = look.red
                canvas.drawCircle(cx, cy, r, fill)
                glyph(canvas, stop, Color.WHITE, cx, cy, r)
            }
            Mode.WORKING -> {
                fill.color = look.fn
                canvas.drawCircle(cx, cy, r, fill)
                glyph(canvas, mic, look.onVariant, cx, cy, r)
                ring.strokeWidth = r * 0.12f
                ring.strokeCap = Paint.Cap.ROUND
                ring.color = withAlpha(look.amber, 60)
                val rr = r * 1.22f
                canvas.drawCircle(cx, cy, rr, ring)
                ring.color = look.amber
                arc.set(cx - rr, cy - rr, cx + rr, cy + rr)
                val start = (t * 300f) % 360f
                val sweep = 60f + 50f * (sin(t * 2.5).toFloat() + 1f)
                canvas.drawArc(arc, start, sweep, false, ring)
                ring.strokeCap = Paint.Cap.BUTT
            }
        }
        if (isShown) postInvalidateOnAnimation()
    }

    private fun glyph(canvas: Canvas, d: Drawable, tint: Int, cx: Float, cy: Float, r: Float) {
        val s = (r * 0.62f).toInt()
        d.colorFilter = PorterDuffColorFilter(tint, PorterDuff.Mode.SRC_IN)
        d.setBounds((cx - s).toInt(), (cy - s).toInt(), (cx + s).toInt(), (cy + s).toInt())
        d.draw(canvas)
    }
}

/**
 * A voz desenhada enquanto grava: barras espelhadas em volta de uma linha, que
 * correm da direita para a esquerda. Cada barra é um décimo de segundo.
 */
class Wave(ctx: Context, private val look: Palette) : View(ctx) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { strokeCap = Paint.Cap.ROUND }
    private val bars = FloatArray(BARS)
    private var head = 0
    private var lastPush = 0L

    fun level(rms: Float) {
        bars[head] = loudness(rms)
        head = (head + 1) % BARS
        lastPush = SystemClock.uptimeMillis()
    }

    fun reset() {
        bars.fill(0f)
        lastPush = 0L
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        val step = width / (BARS - 1f)
        val mid = height / 2f
        paint.strokeWidth = step * 0.55f
        // Desliza suave entre uma leitura e a próxima (elas vêm a cada 100 ms).
        val slide = if (lastPush == 0L) 0f else ((SystemClock.uptimeMillis() - lastPush) / 100f).coerceIn(0f, 1f)
        for (i in 0 until BARS) {
            val v = bars[(head + i) % BARS]
            val x = (i - slide) * step
            if (x < 0) continue
            // As pontas somem aos poucos; a voz recente é a mais forte.
            val edge = minOf(x / (width * 0.18f), 1f)
            val h = maxOf(paint.strokeWidth * 0.5f, v * (mid - paint.strokeWidth))
            paint.color = withAlpha(look.red, (70 + 185 * edge * (0.45f + 0.55f * v)).toInt().coerceAtMost(255))
            canvas.drawLine(x, mid - h, x, mid + h, paint)
        }
        if (isShown) postInvalidateOnAnimation()
    }

    private companion object {
        const val BARS = 44
    }
}

/** RMS de 0 a 1 para 0 a 1 perceptível: -50 dB é nada, -10 dB é tudo. */
private fun loudness(rms: Float): Float {
    val db = 20f * log10(rms.coerceAtLeast(1e-5f))
    return ((db + 50f) / 40f).coerceIn(0f, 1f)
}

private fun withAlpha(color: Int, a: Int) = (color and 0x00FFFFFF) or (a.coerceIn(0, 255) shl 24)
