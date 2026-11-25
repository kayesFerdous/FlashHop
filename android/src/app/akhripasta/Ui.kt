package app.akhripasta

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Canvas
import android.graphics.ColorFilter
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.InsetDrawable
import android.graphics.drawable.RippleDrawable
import android.text.TextUtils
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView

/** Tokens and custom views. desktop/src/ui.rs mirrors the same palette and shapes; change both together. */
object C {
    const val BG = 0xFFE9EBF5.toInt()
    const val CARD = 0xFFFFFFFF.toInt()
    const val RAISED = 0xFFEEF0F7.toInt()
    const val LINE = 0xFFE6E8F0.toInt()
    const val INK = 0xFF0E0F14.toInt()
    const val MUTED = 0xFF585D70.toInt()
    const val COBALT = 0xFF2F55E4.toInt()
    const val LIME = 0xFFD6F55B.toInt()
    const val WHITE = 0xFFFFFFFF.toInt()
    const val ON_COBALT_MUTED = 0xFFDCE3FD.toInt()
    const val GLASS = 0x20FFFFFF
    const val GREEN = 0xFF158A55.toInt()
    const val AMBER = 0xFFA86A0A.toInt()
    const val RED = 0xFFD6333A.toInt()
    const val GREEN_BG = 0xFFDDF5E8.toInt()
    const val AMBER_BG = 0xFFFCF0D6.toInt()
    const val RED_BG = 0xFFFCE4E5.toInt()
    const val PERFORATION = 0xFFB9C7F6.toInt()
}

/** Button surfaces, same names as desktop `Tone`. */
enum class Tone(val fill: Int, val fg: Int, val ripple: Int) {
    Ink(C.INK, C.WHITE, 0x33FFFFFF),
    Card(C.RAISED, C.INK, 0x1A0E0F14),
    Ground(C.WHITE, C.INK, 0x1A0E0F14),
    Glass(C.GLASS, C.WHITE, 0x33FFFFFF),
    Lime(C.LIME, C.INK, 0x220E0F14),
}

enum class Ic { Plus, Close, Check, Up, Down, UpRight, Folder, File, Text, Paste, Sliders }

fun Context.dp(v: Float) = v * resources.displayMetrics.density
fun Context.dp(v: Int) = dp(v.toFloat()).toInt()

fun face(weight: Int): Typeface = Typeface.create(Typeface.DEFAULT, weight, false)

fun Context.tv(s: CharSequence, sp: Float, weight: Int, color: Int, lines: Int = 1) = TextView(this).apply {
    text = s
    setTextSize(TypedValue.COMPLEX_UNIT_SP, sp)
    typeface = face(weight)
    setTextColor(color)
    includeFontPadding = false
    if (lines > 0) { maxLines = lines; ellipsize = TextUtils.TruncateAt.END }
}

fun Context.rounded(color: Int, radiusDp: Float) = GradientDrawable().apply { setColor(color); cornerRadius = dp(radiusDp) }

fun oval(color: Int) = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(color) }

fun Context.row(vararg views: View, gravity: Int = Gravity.CENTER_VERTICAL) = LinearLayout(this).apply {
    this.gravity = gravity
    views.forEach { addView(it) }
}

fun Context.column(vararg views: View) = LinearLayout(this).apply {
    orientation = LinearLayout.VERTICAL
    views.forEach { addView(it) }
}

fun View.weight1() = apply { layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f) }

fun View.margins(l: Int = 0, t: Int = 0, r: Int = 0, b: Int = 0) = apply {
    // Containers fill the width (rows need it for weights); leaf views wrap.
    val lp = (layoutParams as? LinearLayout.LayoutParams) ?: LinearLayout.LayoutParams(
        if (this is LinearLayout) LinearLayout.LayoutParams.MATCH_PARENT else LinearLayout.LayoutParams.WRAP_CONTENT,
        LinearLayout.LayoutParams.WRAP_CONTENT,
    )
    lp.setMargins(l, t, r, b)
    layoutParams = lp
}

/** Pill button with an optional leading icon. */
fun Context.pill(s: String, icon: Ic?, tone: Tone, onClick: () -> Unit) = TextView(this).apply {
    text = s
    setTextSize(TypedValue.COMPLEX_UNIT_SP, 14.5f)
    typeface = face(600)
    setTextColor(tone.fg)
    gravity = Gravity.CENTER
    minHeight = dp(48)
    setPadding(dp(18), 0, dp(18), 0)
    background = RippleDrawable(ColorStateList.valueOf(tone.ripple), rounded(tone.fill, 22f), null)
    if (icon != null) {
        val d = IconDrawable(icon, tone.fg, tone.fill).apply { setBounds(0, 0, dp(17), dp(17)) }
        setCompoundDrawables(d, null, null, null)
        compoundDrawablePadding = dp(7)
    }
    isClickable = true
    setOnClickListener { onClick() }
}

/** Round icon button; disabled shows a grey circle. The touch target is at least 48dp even
 *  when the circle is drawn smaller. */
fun Context.round(icon: Ic, tip: String, sizeDp: Int, tone: Tone, enabled: Boolean = true, onClick: () -> Unit) = View(this).apply {
    val (fill, fg) = if (enabled) tone.fill to tone.fg else C.RAISED to (C.MUTED and 0x99FFFFFF.toInt())
    val hit = maxOf(sizeDp, 48)
    val pad = dp(hit - sizeDp) / 2
    background = InsetDrawable(RippleDrawable(ColorStateList.valueOf(tone.ripple), oval(fill), null), pad)
    foreground = InsetDrawable(IconDrawable(icon, fg, fill), pad + (dp(sizeDp) * 0.27f).toInt())
    contentDescription = tip
    tooltipText = tip
    isEnabled = enabled
    layoutParams = LinearLayout.LayoutParams(dp(hit), dp(hit))
    setOnClickListener { onClick() }
}

/** Icon in a filled circle, not clickable. */
fun Context.badge(icon: Ic, fg: Int, bg: Int, sizeDp: Int) = View(this).apply {
    background = oval(bg)
    foreground = InsetDrawable(IconDrawable(icon, fg, bg), (dp(sizeDp) * 0.28f).toInt())
    layoutParams = LinearLayout.LayoutParams(dp(sizeDp), dp(sizeDp))
    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
}

fun Context.glyph(icon: Ic, color: Int, sizeDp: Int) = View(this).apply {
    background = IconDrawable(icon, color, C.WHITE)
    layoutParams = LinearLayout.LayoutParams(dp(sizeDp), dp(sizeDp))
    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
}

/** First letter of a device name in a circle. */
fun Context.avatar(name: String, sizeDp: Int, bg: Int, fg: Int) = tv(initial(name), sizeDp * 0.42f, 600, fg).apply {
    gravity = Gravity.CENTER
    background = oval(bg)
    layoutParams = LinearLayout.LayoutParams(dp(sizeDp), dp(sizeDp))
    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
    setTextSize(TypedValue.COMPLEX_UNIT_DIP, sizeDp * 0.42f) // fixed: it's a glyph in a fixed circle
}

fun initial(name: String) = name.firstOrNull { it.isLetterOrDigit() }?.uppercase() ?: "?"

fun Context.hairline() = View(this).apply {
    setBackgroundColor(C.LINE)
    layoutParams = LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 1).apply {
        setMargins(0, dp(10), 0, dp(10))
    }
}

/** Icons on a 24-unit grid, one stroke weight; same geometry as desktop `paint()`. */
class IconDrawable(private val icon: Ic, color: Int, private val bg: Int) : Drawable() {
    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE; this.color = color; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND
    }
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { this.color = bg }

    override fun draw(c: Canvas) {
        val b = bounds
        val s = b.width() / 24f
        c.save()
        c.translate(b.left.toFloat(), b.top.toFloat())
        c.scale(s, s)
        stroke.strokeWidth = 1.9f
        fun line(vararg p: Float) {
            val path = Path().apply { moveTo(p[0], p[1]); for (i in 2 until p.size step 2) lineTo(p[i], p[i + 1]) }
            c.drawPath(path, stroke)
        }
        fun closed(vararg p: Float) {
            val path = Path().apply { moveTo(p[0], p[1]); for (i in 2 until p.size step 2) lineTo(p[i], p[i + 1]); close() }
            c.drawPath(path, stroke)
        }
        when (icon) {
            Ic.Plus -> { line(12f, 5f, 12f, 19f); line(5f, 12f, 19f, 12f) }
            Ic.Close -> { line(6.5f, 6.5f, 17.5f, 17.5f); line(17.5f, 6.5f, 6.5f, 17.5f) }
            Ic.Check -> line(5f, 12.5f, 10f, 17.5f, 19f, 7f)
            Ic.Up -> { line(12f, 19f, 12f, 5f); line(6f, 11f, 12f, 5f, 18f, 11f) }
            Ic.Down -> { line(12f, 5f, 12f, 19f); line(6f, 13f, 12f, 19f, 18f, 13f) }
            Ic.UpRight -> { line(7f, 17f, 17f, 7f); line(8f, 7f, 17f, 7f, 17f, 16f) }
            Ic.Folder -> closed(3f, 6f, 9.5f, 6f, 11.5f, 8.5f, 21f, 8.5f, 21f, 19f, 3f, 19f)
            Ic.File -> { closed(6f, 3f, 14f, 3f, 19f, 8f, 19f, 21f, 6f, 21f); line(14f, 3f, 14f, 8f, 19f, 8f) }
            Ic.Text -> { line(5f, 7f, 19f, 7f); line(5f, 12f, 19f, 12f); line(5f, 17f, 13f, 17f) }
            Ic.Paste -> {
                c.drawRoundRect(RectF(5f, 5f, 19f, 21f), 2f, 2f, stroke)
                val clip = RectF(8.5f, 3f, 15.5f, 7f)
                c.drawRoundRect(clip, 1.5f, 1.5f, fill)
                c.drawRoundRect(clip, 1.5f, 1.5f, stroke)
            }
            Ic.Sliders -> {
                line(4f, 8f, 20f, 8f); line(4f, 16f, 20f, 16f)
                for ((x, y) in listOf(15f to 8f, 9f to 16f)) { c.drawCircle(x, y, 2.8f, fill); c.drawCircle(x, y, 2.8f, stroke) }
            }
        }
        c.restore()
    }

    override fun setAlpha(alpha: Int) { stroke.alpha = alpha }
    override fun setColorFilter(cf: ColorFilter?) { stroke.colorFilter = cf }
    @Deprecated("Deprecated in Java")
    override fun getOpacity() = PixelFormat.TRANSLUCENT
}

/** White ticket with side notches cut out (true holes, so whatever is behind shows through)
 *  and a dashed perforation between [top] and [stub]. */
class Ticket(ctx: Context) : LinearLayout(ctx) {
    val top = LinearLayout(ctx).apply { orientation = VERTICAL }
    val stub = LinearLayout(ctx).apply { orientation = VERTICAL }
    private val paper = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = C.WHITE }
    private val dash = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE; color = C.PERFORATION; strokeWidth = ctx.dp(1.5f)
        pathEffect = DashPathEffect(floatArrayOf(ctx.dp(6f), ctx.dp(5f)), 0f)
    }

    init {
        orientation = VERTICAL
        setWillNotDraw(false)
        setPadding(ctx.dp(18), ctx.dp(16), ctx.dp(18), ctx.dp(16))
        addView(top)
        addView(stub.margins(t = ctx.dp(26)))
    }

    override fun onDraw(c: Canvas) {
        val w = width.toFloat()
        val y = top.bottom + context.dp(13f)
        val r = context.dp(11f)
        val shape = Path().apply { addRoundRect(RectF(0f, 0f, w, height.toFloat()), context.dp(22f), context.dp(22f), Path.Direction.CW) }
        shape.op(Path().apply { addCircle(0f, y, r, Path.Direction.CW); addCircle(w, y, r, Path.Direction.CW) }, Path.Op.DIFFERENCE)
        c.drawPath(shape, paper)
        c.drawLine(context.dp(18f), y, w - context.dp(18f), y, dash)
    }
}

/** Progress as thin ticks: lit in cobalt, the leading tick in ink. */
class Ticks(ctx: Context) : View(ctx) {
    var fraction = 0f
        set(v) { field = v; invalidate() }
    private val p = Paint(Paint.ANTI_ALIAS_FLAG)

    override fun onMeasure(w: Int, h: Int) = setMeasuredDimension(MeasureSpec.getSize(w), context.dp(20))

    override fun onDraw(c: Canvas) {
        val bar = context.dp(3f)
        val pitch = context.dp(6f)
        val n = ((width + bar) / pitch).toInt().coerceAtLeast(1)
        val lit = Math.round(fraction.coerceIn(0f, 1f) * n)
        for (i in 0 until n) {
            p.color = if (i + 1 == lit) C.INK else if (i < lit) C.COBALT else 0xFFE2E5F0.toInt()
            val x = i * pitch
            c.drawRoundRect(RectF(x, 0f, x + bar, height.toFloat()), bar / 1.5f, bar / 1.5f, p)
        }
    }
}

/** A barcode drawn from [seed], so the same payload always prints the same code. */
class Barcode(ctx: Context, private val seed: Long) : View(ctx) {
    private val p = Paint().apply { color = C.INK }

    override fun onDraw(c: Canvas) {
        val u = context.dp(1f)
        var x = 0f
        var s = seed or 1L
        while (x < width - 3 * u) {
            s = s xor (s shl 13); s = s xor (s ushr 7); s = s xor (s shl 17)
            val bar = (1 + (s and 3L).toInt() % 3) * u
            val gap = (1.5f + ((s ushr 8) and 3L).toInt() % 3) * u
            c.drawRect(x, 0f, minOf(x + bar, width.toFloat()), height.toFloat(), p)
            x += bar + gap
        }
    }
}
