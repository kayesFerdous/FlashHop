package app.flashhop

import android.Manifest
import android.app.Activity
import android.app.DownloadManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import android.provider.OpenableColumns
import android.provider.Settings
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.AbsoluteSizeSpan
import android.text.style.ForegroundColorSpan
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.animation.AccelerateInterpolator
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import android.window.OnBackInvokedCallback
import android.window.OnBackInvokedDispatcher
import java.io.File
import kotlin.concurrent.thread

/** A file (content uri), folder or text waiting to be sent. [image]: pasted from the clipboard, so it
 *  lands on the other device's clipboard too. [files]: a picked folder's files as (uri, "Folder/sub/name"). */
class Staged(
    val uri: Uri?, val text: String?, val label: String, val size: Long?, val image: Boolean = false,
    val files: List<Pair<Uri, String>>? = null,
)

class MainActivity : Activity() {
    private val staged = ArrayList<Staged>()
    private lateinit var prefs: android.content.SharedPreferences
    private lateinit var glow: View
    private lateinit var nameView: TextView
    private lateinit var avatarView: TextView
    private lateinit var headline: TextView
    private lateinit var errorView: TextView
    private lateinit var offersBox: LinearLayout
    private lateinit var dockBox: LinearLayout
    private lateinit var devicesCard: LinearLayout
    private lateinit var activityCard: LinearLayout
    private lateinit var col: LinearLayout
    private lateinit var scroll: ScrollView
    private lateinit var findInput: EditText
    private lateinit var devicesHead: LinearLayout
    private lateinit var devicesRows: LinearLayout
    /** Where received files go: a plain path under Download/ or Documents/. */
    private lateinit var dir: File
    /** The settings page while it replaces the main column. */
    private var settings: View? = null
    private var dirView: TextView? = null
    private var backCallback: Any? = null // OnBackInvokedCallback on Android 13+
    private var multicast: WifiManager.MulticastLock? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        prefs = getSharedPreferences("flashhop", MODE_PRIVATE)
        val name = prefs.getString("name", null)
            ?: Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME) ?: Build.MODEL
        @Suppress("DEPRECATION")
        dir = prefs.getString("dir", null)?.let(::File)
            ?: File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "FlashHop")

        // Soft cobalt wash from the top-left; brighter while something is offered or moving.
        glow = View(this).apply {
            background = GradientDrawable().apply {
                gradientType = GradientDrawable.RADIAL_GRADIENT
                colors = intArrayOf(C.COBALT and 0x66FFFFFF, C.COBALT and 0x22FFFFFF, C.COBALT and 0x00FFFFFF)
                setGradientCenter(0.2f, 0f)
                gradientRadius = dp(460f)
            }
            alpha = 0.6f
        }

        avatarView = avatar(name, 44, C.INK, C.LIME)
        nameView = tv(name, 16f, 600, C.INK)
        val top = row(
            avatarView,
            column(nameView, tv("Visible on this network", 13f, 500, 0xFF3D4152.toInt()).margins(t = dp(3))).weight1().margins(l = dp(12)),
            round(Ic.Folder, "Received files", 48, Tone.Ground) { openDir() },
            round(Ic.Sliders, "Settings", 48, Tone.Ground) { openSettings() }.margins(l = dp(8)),
        )
        headline = tv("", 30f, 650, C.INK, lines = 0).apply { letterSpacing = -0.02f; setLineSpacing(0f, 1.08f) }
        errorView = tv("", 14f, 500, C.RED, lines = 0).apply {
            background = rounded(C.RED_BG, 16f); setPadding(dp(14), dp(12), dp(14), dp(12)); visibility = View.GONE
        }
        offersBox = column()
        dockBox = column().apply { background = rounded(C.COBALT, 28f); setPadding(dp(14), dp(14), dp(14), dp(14)) }
        findInput = field("Find a device").apply {
            imeOptions = EditorInfo.IME_ACTION_GO
            onChange { render() }
            // Go with exactly one match sends to it, like Enter on desktop.
            setOnEditorActionListener { _, _, _ ->
                val m = matching(snap.peers)
                if (staged.isNotEmpty() && m.size == 1) send(m[0]) else hideKeyboard()
                true
            }
        }
        devicesHead = column()
        devicesRows = column()
        devicesCard = card().apply { addView(devicesHead); addView(findInput.margins(b = dp(10))); addView(devicesRows) }
        activityCard = card()

        col = column(
            top, headline.margins(t = dp(28)), errorView.margins(t = dp(16)), offersBox.margins(t = dp(20)),
            dockBox, devicesCard.margins(t = dp(14)), activityCard.margins(t = dp(14)),
        ).apply { setPadding(dp(20), dp(12), dp(20), dp(28)) }
        scroll = ScrollView(this).apply { addView(col); isVerticalScrollBarEnabled = false }
        val root = FrameLayout(this).apply {
            setBackgroundColor(C.BG)
            addView(glow, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(520)))
            addView(scroll)
        }
        // Edge to edge: the glow runs under the status bar, content stays clear of bars and keyboard.
        scroll.setOnApplyWindowInsetsListener { v, insets ->
            val i = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.ime())
            v.setPadding(i.left, i.top, i.right, i.bottom)
            insets
        }
        setContentView(root)
        val light = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
        window.insetsController?.setSystemBarsAppearance(light, light)
        window.isNavigationBarContrastEnforced = false

        Core.start(name, dir.path, prefs.getString("recent", "")!!, filesDir.path)?.let { errorView.text = it; errorView.visibility = View.VISIBLE }
        startWatcher(applicationContext)
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }
        handleShare(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleShare(intent)
    }

    override fun onStart() {
        super.onStart()
        // Broadcast discovery only needs to listen while the app is visible; saves battery.
        multicast = (applicationContext.getSystemService(WIFI_SERVICE) as WifiManager)
            .createMulticastLock("flashhop").apply { setReferenceCounted(false); acquire() }
    }

    override fun onStop() {
        multicast?.release()
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        ui = this
        getSystemService(NotificationManager::class.java).cancelAll()
        render()
    }

    override fun onPause() {
        ui = null
        super.onPause()
    }

    // ---------- staging ----------

    private fun handleShare(intent: Intent?) {
        when (intent?.action) {
            Intent.ACTION_SEND -> {
                @Suppress("DEPRECATION")
                val uri = intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
                if (uri != null) stageUri(uri) else intent.getStringExtra(Intent.EXTRA_TEXT)?.let { stageText(it) }
            }
            Intent.ACTION_SEND_MULTIPLE -> {
                @Suppress("DEPRECATION")
                intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.forEach { stageUri(it) }
            }
        }
        intent?.action = null // don't stage again on rotation/resume
        render()
    }

    private fun pickFiles() {
        val i = Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE)
            .putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
        @Suppress("DEPRECATION")
        startActivityForResult(i, PICK)
    }

    private fun pickFolder() {
        @Suppress("DEPRECATION")
        startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE), PICK_FOLDER)
    }

    /** Lists the folder off the UI thread (big folders take a moment), then stages it as one item. */
    private fun stageFolder(tree: Uri) = thread {
        val root = DocumentsContract.getTreeDocumentId(tree)
        val files = ArrayList<Triple<Uri, String, Long>>()
        val name = runCatching {
            val n = contentResolver.query(DocumentsContract.buildDocumentUriUsingTree(tree, root),
                arrayOf(Document.COLUMN_DISPLAY_NAME), null, null, null)?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
            cleanName(n ?: root.substringAfterLast('/').substringAfterLast(':')).also { walkTree(tree, root, it, files, 0) }
        }
        runOnUiThread {
            name.onFailure { toast("Couldn't read that folder: ${it.message}") }.onSuccess {
                if (files.isEmpty()) toast("That folder has no files")
                else staged += Staged(tree, null, it, files.sumOf { f -> f.third }, files = files.map { f -> f.first to f.second })
            }
            render()
        }
    }

    /** Every file under [docId] as (document uri, "path/name", size). Empty folders are skipped, like desktop. */
    private fun walkTree(tree: Uri, docId: String, path: String, out: ArrayList<Triple<Uri, String, Long>>, depth: Int) {
        if (depth > 32) return // a symlink loop would otherwise recurse forever
        val cols = arrayOf(Document.COLUMN_DOCUMENT_ID, Document.COLUMN_DISPLAY_NAME, Document.COLUMN_MIME_TYPE, Document.COLUMN_SIZE)
        contentResolver.query(DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId), cols, null, null, null)?.use { c ->
            while (c.moveToNext()) {
                val id = c.getString(0) ?: continue
                val rel = path + "/" + cleanName(c.getString(1) ?: continue)
                if (c.getString(2) == Document.MIME_TYPE_DIR) walkTree(tree, id, rel, out, depth + 1)
                else out += Triple(DocumentsContract.buildDocumentUriUsingTree(tree, id), rel, if (c.isNull(3)) 0L else c.getLong(3))
            }
        }
    }

    /** One path component: no '/', never "." or "..". The receiver checks again; this keeps the folder intact. */
    private fun cleanName(n: String) = n.replace('/', '_').let { if (it.isEmpty() || it == "." || it == "..") "_" else it }

    @Deprecated("Platform API; no AndroidX on purpose")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (resultCode != RESULT_OK || data == null) return
        if (requestCode == PICK_DIR) { data.data?.let { setDir(it) }; return }
        if (requestCode == PICK_FOLDER) { data.data?.let { stageFolder(it) }; return }
        if (requestCode != PICK) return
        val clip = data.clipData
        if (clip != null) for (i in 0 until clip.itemCount) stageUri(clip.getItemAt(i).uri) else data.data?.let { stageUri(it) }
        render()
    }

    private fun paste() {
        val clip = getSystemService(ClipboardManager::class.java).primaryClip
        if (clip == null || clip.itemCount == 0) return toast("Clipboard is empty")
        for (i in 0 until clip.itemCount) {
            val item = clip.getItemAt(i)
            val uri = item.uri
            if (uri == null) stageText(item.coerceToText(this).toString())
            else stageUri(uri, image = contentResolver.getType(uri)?.startsWith("image/") ?: clip.description.hasMimeType("image/*"))
        }
        render()
    }

    private fun stageUri(uri: Uri, image: Boolean = false) {
        var name: String? = null
        var size: Long? = null
        runCatching {
            contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
                if (c.moveToFirst()) { name = c.getString(0); if (!c.isNull(1)) size = c.getLong(1) }
            }
        }
        staged += Staged(uri, null, name ?: uri.lastPathSegment ?: "file", size, image)
    }

    /** A clipboard image as a PNG fd: as-is if it already is one, else re-encoded through an
     *  unlinked cache file, so every receiver can put it on its clipboard. */
    private fun pngFd(uri: Uri): Int {
        if (contentResolver.getType(uri) == "image/png") return contentResolver.openFileDescriptor(uri, "r")!!.detachFd()
        val bmp = ImageDecoder.decodeBitmap(ImageDecoder.createSource(contentResolver, uri)) { d, _, _ ->
            d.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        }
        val f = File.createTempFile("clip", ".png", cacheDir)
        try {
            f.outputStream().use { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }
            return ParcelFileDescriptor.open(f, ParcelFileDescriptor.MODE_READ_ONLY).detachFd()
        } finally {
            f.delete()
        }
    }

    private fun stageText(t: String) {
        if (t.isNotEmpty()) staged += Staged(null, t, t.take(80).replace(Regex("\\s+"), " "), t.toByteArray().size.toLong())
    }

    private fun send(peer: Peer) {
        val fds = ArrayList<Int>()
        val names = ArrayList<String>()
        val texts = ArrayList<String>()
        var images = 0
        // Clipboard images go first: Core.send treats the first `images` fds as clipboard images.
        for (s in staged.sortedByDescending { it.image }) {
            if (s.text != null) { texts += s.text; continue }
            if (s.files != null) {
                for ((uri, rel) in s.files) runCatching { contentResolver.openFileDescriptor(uri, "r")!!.detachFd() }
                    .onSuccess { fds += it; names += rel }.onFailure { toast("Skipped $rel: ${it.message}") }
                continue
            }
            val png = if (s.image) runCatching { pngFd(s.uri!!) }.getOrNull() else null
            if (png != null) { fds += png; names += s.label.substringBeforeLast('.') + ".png"; images++; continue }
            val fd = runCatching { contentResolver.openFileDescriptor(s.uri!!, "r")!!.detachFd() }
            fd.onSuccess { fds += it; names += s.label }.onFailure { toast("Skipped ${s.label}: ${it.message}") }
        }
        if (fds.isEmpty() && texts.isEmpty()) return
        Core.send(peer.id, fds.toIntArray(), names.toTypedArray(), texts.toTypedArray(), images)
        // The ticket slides out over the page (ease-in, fading) while the dock re-renders under it.
        // Mirrored in desktop App::ui.
        (dockBox.getChildAt(0) as? Ticket)?.let { t ->
            val host = window.decorView as ViewGroup
            host.overlay.add(t)
            t.animate().translationX(t.width * 0.6f).alpha(0f).setDuration(320)
                .setInterpolator(AccelerateInterpolator()).withEndAction { host.overlay.remove(t) }.start()
        }
        staged.clear()
        saveRecent()
        hideKeyboard()
        findInput.text.clear()
        render()
    }

    private fun saveRecent() = prefs.edit().putString("recent", Core.recent()).apply()

    private fun hideKeyboard() = getSystemService(InputMethodManager::class.java).hideSoftInputFromWindow(scroll.windowToken, 0)

    // ---------- settings ----------

    /** Settings replace the main column, as on desktop: device name and where received files go. */
    private fun openSettings() {
        if (settings != null) return
        val nameField = field("Device name").apply { setText(nameView.text); onChange { rename(it) } }
        dirView = tv(shortDir(), 16f, 500, C.INK, lines = 0)
        val version = runCatching { packageManager.getPackageInfo(packageName, 0).versionName }.getOrNull().orEmpty()
        val page = column(
            row(round(Ic.Left, "Back", 48, Tone.Ground) { closeSettings() }, tv("Settings", 26f, 650, C.INK).margins(l = dp(12))),
            card().apply {
                addView(tv("Device name", 13f, 500, C.MUTED))
                addView(nameField.margins(t = dp(8)))
                addView(tv("Other devices see this name.", 13f, 400, C.MUTED).margins(t = dp(8)))
            }.margins(t = dp(24)),
            card().apply {
                addView(tv("Received files", 13f, 500, C.MUTED))
                addView(dirView!!.margins(t = dp(4)))
                addView(row(
                    pill("Change…", Ic.Folder, Tone.Ink) { pickDir() },
                    pill("Open", null, Tone.Card) { openDir() }.margins(l = dp(8)),
                ).margins(t = dp(12)))
                addView(tv("Any folder inside Download or Documents.", 13f, 400, C.MUTED).margins(t = dp(10)))
            }.margins(t = dp(12)),
            tv("FlashHop $version", 12f, 400, C.MUTED).margins(l = dp(4), t = dp(16)),
        ).apply { setPadding(dp(20), dp(12), dp(20), dp(28)) }
        settings = page
        scroll.removeAllViews()
        scroll.addView(page)
        if (Build.VERSION.SDK_INT >= 33) {
            val cb = OnBackInvokedCallback { closeSettings() }
            onBackInvokedDispatcher.registerOnBackInvokedCallback(OnBackInvokedDispatcher.PRIORITY_DEFAULT, cb)
            backCallback = cb
        }
    }

    private fun closeSettings() {
        if (settings == null) return
        hideKeyboard()
        if (Build.VERSION.SDK_INT >= 33) (backCallback as? OnBackInvokedCallback)?.let { onBackInvokedDispatcher.unregisterOnBackInvokedCallback(it) }
        backCallback = null
        settings = null
        dirView = null
        scroll.removeAllViews()
        scroll.addView(col)
        render()
    }

    @Deprecated("Back on Android 11 and 12; 13+ uses the OnBackInvokedCallback in openSettings()")
    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        if (settings != null) closeSettings() else super.onBackPressed()
    }

    private fun rename(raw: String) {
        val n = raw.trim()
        if (n.isEmpty()) return
        prefs.edit().putString("name", n).apply()
        Core.setName(n)
        nameView.text = n
        avatarView.text = initial(n)
    }

    @Suppress("DEPRECATION")
    private fun shortDir() = dir.path.removePrefix(Environment.getExternalStorageDirectory().path + "/")

    private fun dirUri() = DocumentsContract.buildDocumentUri("com.android.externalstorage.documents", "primary:" + shortDir())

    private fun pickDir() {
        val i = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).putExtra(DocumentsContract.EXTRA_INITIAL_URI, dirUri())
        @Suppress("DEPRECATION")
        startActivityForResult(i, PICK_DIR)
    }

    /** The engine saves by plain path. Android 11+ lets apps write any file type by path only under
     *  Download/ and Documents/ on internal storage, so only folders there are accepted. */
    @Suppress("DEPRECATION")
    private fun setDir(tree: Uri) {
        val id = DocumentsContract.getTreeDocumentId(tree)
        val rel = id.substringAfter(':', "")
        if (!id.startsWith("primary:") || rel.substringBefore('/') !in setOf("Download", "Documents")) {
            return toast("Pick a folder inside Download or Documents")
        }
        dir = File(Environment.getExternalStorageDirectory(), rel)
        prefs.edit().putString("dir", dir.path).apply()
        Core.start(nameView.text.toString(), dir.path, "", filesDir.path) // running engine: just switches the folder
        dirView?.text = shortDir()
        render()
    }

    /** Opens the folder in the Files app; falls back to the Downloads list. */
    private fun openDir() {
        dir.mkdirs()
        val view = Intent(Intent.ACTION_VIEW).setDataAndType(dirUri(), DocumentsContract.Document.MIME_TYPE_DIR)
        runCatching { startActivity(view) }.onFailure { startActivity(Intent(DownloadManager.ACTION_VIEW_DOWNLOADS)) }
    }

    // ---------- rendering ----------

    private fun card() = column().apply { background = rounded(C.CARD, 24f); setPadding(dp(18), dp(16), dp(18), dp(16)) }

    /** Two-tone headline that names the current step; wording comes from core. */
    private fun headlineText(peers: Int, items: Int): CharSequence {
        val (strong, rest) = Core.headline(peers, items).split('\u001f')
        return SpannableStringBuilder(strong + rest).apply {
            setSpan(ForegroundColorSpan(C.MUTED), strong.length, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
    }

    fun render() {
        val snap = snap
        headline.text = headlineText(snap.peers.size, staged.size)
        val target = when {
            snap.offers.isNotEmpty() -> 1f
            snap.jobs.any { it.active } -> 0.85f
            snap.peers.isEmpty() -> 0.5f
            else -> 0.65f
        }
        if (glow.alpha != target) glow.animate().alpha(target).setDuration(600).start()

        offersBox.removeAllViews()
        for (o in snap.offers) offersBox.addView(offerTicket(o).margins(b = dp(14)))
        // While something is offered or moving and nothing is staged, the dock shrinks to one
        // row and live transfers take its place above Devices.
        val live = snap.jobs.any { it.active }
        renderDock(staged.isEmpty() && (live || snap.offers.isNotEmpty()))
        renderDevices(snap.peers)
        renderActivity(snap.jobs)
        val (first, second) = if (live) activityCard to devicesCard else devicesCard to activityCard
        if (col.indexOfChild(first) > col.indexOfChild(second)) {
            col.removeView(first)
            col.addView(first, col.indexOfChild(second))
        }
    }

    private fun offerTicket(o: Offer) = Ticket(this).apply {
        top.addView(row(badge(Ic.Down, C.LIME, C.INK, 44), tv(o.summary, 16f, 500, C.INK, lines = 0).weight1().margins(l = dp(12))))
        stub.addView(row(
            column(tv("Saves to", 12f, 400, C.MUTED), tv(shortDir(), 14f, 500, C.INK).margins(t = dp(3))).weight1(),
            pill("Decline", null, Tone.Card) { Core.decide(o.id, false) },
            pill("Accept", Ic.Check, Tone.Ink) { Core.decide(o.id, true); saveRecent() }.margins(l = dp(8)),
        ))
    }

    private fun renderDock(compact: Boolean) {
        dockBox.removeAllViews()
        if (compact) {
            dockBox.addView(row(
                pill("Files", Ic.File, Tone.Glass) { pickFiles() },
                pill("Folder", Ic.Folder, Tone.Glass) { pickFolder() }.margins(l = dp(8)),
                pill("Paste", Ic.Paste, Tone.Glass) { paste() }.margins(l = dp(8)),
                gravity = Gravity.CENTER,
            ))
            return
        }
        if (staged.isEmpty()) {
            dockBox.addView(column(
                round(Ic.Plus, "Pick files", 58, Tone.Lime) { pickFiles() }.margins(t = dp(14)),
                tv("Share or pick files", 18f, 600, C.WHITE).margins(t = dp(14)),
                tv("Or tap Share → FlashHop in any app", 14f, 400, C.ON_COBALT_MUTED).margins(t = dp(6), b = dp(16)),
            ).apply { gravity = Gravity.CENTER_HORIZONTAL; layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT) })
        } else {
            dockBox.addView(outgoingTicket().margins(b = dp(12)))
        }
        dockBox.addView(row(
            pill("Files", Ic.File, Tone.Glass) { pickFiles() },
            pill("Folder", Ic.Folder, Tone.Glass) { pickFolder() }.margins(l = dp(8)),
            pill("Paste", Ic.Paste, Tone.Glass) { paste() }.margins(l = dp(8)),
            gravity = Gravity.CENTER,
        ).apply { layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT) })
    }

    private fun outgoingTicket() = Ticket(this).apply {
        top.addView(row(
            tv("Outgoing", 20f, 600, C.INK).weight1(),
            round(Ic.Close, "Clear all", 40, Tone.Card) { staged.clear(); render() },
        ))
        for (s in staged.take(MAX_ROWS)) {
            val size = s.size?.let { human(it) } ?: ""
            top.addView(row(
                glyph(if (s.text != null) Ic.Text else if (s.files != null) Ic.Folder else Ic.File, C.MUTED, 20),
                tv(s.label, 15f, 500, C.INK).weight1().margins(l = dp(10), r = dp(8)),
                tv(size, 13f, 400, C.MUTED),
                round(Ic.Close, "Remove ${s.label}", 40, Tone.Ground) { staged.remove(s); render() }.margins(l = dp(4)),
            ).margins(t = dp(6)))
        }
        if (staged.size > MAX_ROWS) top.addView(tv("+${staged.size - MAX_ROWS} more", 14f, 500, C.MUTED).margins(t = dp(6)))
        val total = staged.mapNotNull { it.size }.sum()
        stub.addView(row(
            column(
                tv("Items", 12f, 400, C.MUTED),
                tv("${staged.size}", 20f, 600, C.INK).margins(t = dp(3)),
            ).weight1(),
            column(
                tv(if (staged.any { it.size == null }) "Total, some unknown" else "Total", 12f, 400, C.MUTED),
                tv(if (total > 0) human(total) else "—", 20f, 600, C.INK).margins(t = dp(3)),
            ).apply { gravity = Gravity.END },
            gravity = Gravity.BOTTOM,
        ))
    }

    private fun sectionTitle(title: String, count: String?, action: View? = null) = row(
        tv(title, 17f, 600, C.INK),
        tv(count ?: "", 17f, 400, C.MUTED).weight1().margins(l = dp(6)),
        *listOfNotNull(action).toTypedArray(),
    ).margins(b = dp(8))

    /** Name (any case) or address. Mirrored in desktop `devices_ui`. */
    private fun matching(peers: List<Peer>): List<Peer> {
        val q = findInput.text.toString().trim()
        return peers.filter { it.name.contains(q, ignoreCase = true) || it.ip.contains(q) }
    }

    /** The search field stays put (it keeps focus and the keyboard); only the header and rows rebuild. */
    private fun renderDevices(peers: List<Peer>) {
        devicesHead.removeAllViews()
        devicesHead.addView(sectionTitle("Devices", peers.size.takeIf { it > 0 }?.toString()))
        findInput.visibility = if (peers.size >= FIND_FROM || findInput.text.isNotEmpty()) View.VISIBLE else View.GONE
        devicesRows.removeAllViews()
        if (peers.isEmpty()) {
            devicesRows.addView(tv("Nothing here yet. Open FlashHop on another device on the same network and it appears within a few seconds.", 14f, 400, C.MUTED, lines = 0))
        }
        val shown = matching(peers)
        if (peers.isNotEmpty() && shown.isEmpty()) devicesRows.addView(tv("No device matches.", 14f, 400, C.MUTED))
        val ready = staged.isNotEmpty()
        shown.forEachIndexed { i, p ->
            if (i > 0) devicesRows.addView(hairline())
            devicesRows.addView(row(
                avatar(p.name, 44, C.RAISED, C.INK),
                column(tv(p.name, 16f, 600, C.INK), tv(p.detail, 13f, 400, C.MUTED).margins(t = dp(3))).weight1().margins(l = dp(12), r = dp(8)),
                round(Ic.UpRight, if (ready) "Send to ${p.name}" else "Add files or text first", 52, Tone.Lime, ready) { send(p) },
            ))
        }
    }

    private fun renderActivity(jobs: List<Job>) {
        activityCard.removeAllViews()
        val clear = if (jobs.any { !it.active }) tv("Clear finished", 14f, 500, C.MUTED).apply {
            minHeight = dp(48); gravity = Gravity.CENTER_VERTICAL; setPadding(dp(12), 0, 0, 0)
            setOnClickListener { Core.clearFinished() }
        } else null
        activityCard.addView(sectionTitle("Activity", null, clear))
        if (jobs.isEmpty()) activityCard.addView(tv("Nothing sent or received yet.", 14f, 400, C.MUTED))
        jobs.forEachIndexed { i, j ->
            if (i > 0) activityCard.addView(hairline())
            val arrow = if (j.incoming) Ic.Down else Ic.Up
            val (icon, fg, bg) = when (j.state) {
                'w' -> Triple(arrow, C.AMBER, C.AMBER_BG)
                'r' -> Triple(arrow, C.WHITE, C.COBALT)
                'd' -> Triple(Ic.Check, C.GREEN, C.GREEN_BG)
                else -> Triple(Ic.Close, C.RED, C.RED_BG)
            }
            activityCard.addView(row(
                badge(icon, fg, bg, 44),
                column(tv(j.label, 15f, 600, C.INK), tv(j.status, 13f, 400, if (j.failed) C.RED else C.MUTED).margins(t = dp(3)))
                    .weight1().margins(l = dp(12), r = dp(8)),
                *listOfNotNull(if (j.active) round(Ic.Close, "Cancel", 40, Tone.Card) { Core.cancel(j.id) } else null).toTypedArray(),
            ))
            if (j.state == 'r') {
                val pct = (j.fraction * 100).toInt().toString()
                val num = SpannableStringBuilder("$pct%").apply {
                    setSpan(AbsoluteSizeSpan(16, true), pct.length, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                    setSpan(ForegroundColorSpan(C.MUTED), pct.length, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                }
                activityCard.addView(row(
                    tv(num, 34f, 300, C.INK),
                    Ticks(this).apply { fraction = j.fraction }.weight1().margins(l = dp(12)),
                ).margins(t = dp(8)))
            }
        }
    }

    private fun toast(s: String) = Toast.makeText(this, s, Toast.LENGTH_SHORT).show()

    companion object {
        private const val PICK = 1
        private const val PICK_DIR = 2
        private const val PICK_FOLDER = 3
        /** The device search shows from this many devices on. Mirrored in desktop main.rs. */
        private const val FIND_FROM = 5
        private const val MAX_ROWS = 6
        @Volatile var ui: MainActivity? = null
        @Volatile var snap = Snap(emptyArray())
        private var watching = false

        /** Same as core `human()`, for sizes of staged items before they reach the engine. */
        fun human(b: Long): String {
            val units = arrayOf("B", "KB", "MB", "GB", "TB")
            var v = b.toDouble()
            var i = 0
            while (v >= 1024 && i < units.size - 1) { v /= 1024; i++ }
            return if (i == 0) "$b B" else "%.1f %s".format(v, units[i])
        }

        /** One background thread per process: waits on the engine, refreshes the UI,
         *  copies received text, and notifies about offers while the app isn't open. */
        fun startWatcher(app: Context) {
            synchronized(this) { if (watching) return; watching = true }
            val main = Handler(Looper.getMainLooper())
            val nm = app.getSystemService(NotificationManager::class.java)
            nm.createNotificationChannel(NotificationChannel("offers", "Incoming files", NotificationManager.IMPORTANCE_HIGH))
            var lastOffer = 0L
            thread(isDaemon = true, name = "flashhop-watch") {
                var seen = 0L
                while (true) {
                    seen = Core.waitChange(seen, if (snap.jobs.any { it.active }) 250 else 30_000)
                    val s = Snap(Core.snapshot())
                    snap = s
                    main.post {
                        val cm = app.getSystemService(ClipboardManager::class.java)
                        for (t in s.texts) {
                            cm.setPrimaryClip(ClipData.newPlainText("FlashHop", t))
                            Toast.makeText(app, "Text received and copied", Toast.LENGTH_SHORT).show()
                        }
                        for (p in s.images) {
                            cm.setPrimaryClip(ClipData.newUri(app.contentResolver, "FlashHop", ClipProvider.share(app, File(p))))
                            Toast.makeText(app, "Image received and copied", Toast.LENGTH_SHORT).show()
                        }
                        for (o in s.offers.filter { it.id > lastOffer }) {
                            lastOffer = o.id
                            if (ui == null) notifyOffer(app, nm, o)
                        }
                        ui?.render()
                    }
                }
            }
        }

        private fun notifyOffer(app: Context, nm: NotificationManager, o: Offer) {
            val open = PendingIntent.getActivity(
                app, 0, Intent(app, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK), PendingIntent.FLAG_IMMUTABLE,
            )
            val n = Notification.Builder(app, "offers")
                // No generated R class (see build.sh), so look the drawable up by name.
                .setSmallIcon(app.resources.getIdentifier("ic_stat", "drawable", app.packageName))
                .setColor(C.COBALT)
                .setContentTitle("Incoming files")
                .setContentText(o.summary)
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
            nm.notify(o.id.toInt(), n)
        }
    }
}
