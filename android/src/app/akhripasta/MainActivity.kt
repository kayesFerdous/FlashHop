package app.akhripasta

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
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.OpenableColumns
import android.provider.Settings
import android.text.Editable
import android.text.TextWatcher
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import java.io.File
import kotlin.concurrent.thread

/** A file (content uri) or text waiting to be sent. */
class Staged(val uri: Uri?, val text: String?, val label: String)

class MainActivity : Activity() {
    private val staged = ArrayList<Staged>()
    private lateinit var errorView: TextView
    private lateinit var offersBox: LinearLayout
    private lateinit var stagedBox: LinearLayout
    private lateinit var devicesBox: LinearLayout
    private lateinit var jobsBox: LinearLayout
    private var multicast: WifiManager.MulticastLock? = null

    private val Int.dp get() = (this * resources.displayMetrics.density).toInt()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val prefs = getSharedPreferences("akhri", MODE_PRIVATE)
        val name = prefs.getString("name", null)
            ?: Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME) ?: Build.MODEL
        @Suppress("DEPRECATION")
        val dir = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "AkhriPasta")

        val col = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(16.dp, 8.dp, 16.dp, 16.dp) }
        errorView = TextView(this).apply { setTextColor(Color.rgb(0xd3, 0x2f, 0x2f)); visibility = View.GONE }
        col.addView(errorView)
        offersBox = box(col)
        col.addView(row(heading("To send"), button("Files") { pickFiles() }, button("Paste") { paste() }, button("Clear") { staged.clear(); renderStaged() }))
        stagedBox = box(col)
        col.addView(heading("Devices"))
        devicesBox = box(col)
        col.addView(row(heading("Transfers"), button("Downloads") { startActivity(Intent(DownloadManager.ACTION_VIEW_DOWNLOADS)) }, button("Clear done") { Core.clearFinished() }))
        jobsBox = box(col)
        col.addView(heading("Settings"))
        col.addView(EditText(this).apply {
            setText(name)
            hint = "Device name"
            isSingleLine = true
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
                override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
                override fun afterTextChanged(s: Editable?) {
                    val n = s.toString().trim()
                    if (n.isNotEmpty()) {
                        prefs.edit().putString("name", n).apply()
                        Core.setName(n)
                    }
                }
            })
        })
        col.addView(TextView(this).apply { text = "Received files go to Download/AkhriPasta." })

        val scroll = ScrollView(this).apply { addView(col) }
        // targetSdk 35+ draws edge to edge: keep content clear of the status bar, nav bar and keyboard.
        scroll.setOnApplyWindowInsetsListener { v, insets ->
            val i = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.ime())
            v.setPadding(i.left, i.top, i.right, i.bottom)
            insets
        }
        setContentView(scroll)
        // Dark status/nav bar icons on the light theme, so they stay readable.
        if (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK != Configuration.UI_MODE_NIGHT_YES) {
            val light = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
            window.insetsController?.setSystemBarsAppearance(light, light)
        }

        Core.start(name, dir.path)?.let { errorView.text = it; errorView.visibility = View.VISIBLE }
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
            .createMulticastLock("akhri").apply { setReferenceCounted(false); acquire() }
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
        renderStaged()
    }

    private fun pickFiles() {
        val i = Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE)
            .putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
        @Suppress("DEPRECATION")
        startActivityForResult(i, PICK)
    }

    @Deprecated("Platform API; no AndroidX on purpose")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != PICK || resultCode != RESULT_OK || data == null) return
        val clip = data.clipData
        if (clip != null) for (i in 0 until clip.itemCount) stageUri(clip.getItemAt(i).uri) else data.data?.let { stageUri(it) }
        renderStaged()
    }

    private fun paste() {
        val clip = getSystemService(ClipboardManager::class.java).primaryClip
        if (clip == null || clip.itemCount == 0) return toast("Clipboard is empty")
        for (i in 0 until clip.itemCount) {
            val item = clip.getItemAt(i)
            if (item.uri != null) stageUri(item.uri) else stageText(item.coerceToText(this).toString())
        }
        renderStaged()
    }

    private fun stageUri(uri: Uri) {
        staged += Staged(uri, null, displayName(uri))
    }

    private fun stageText(t: String) {
        if (t.isNotEmpty()) staged += Staged(null, t, "Text: " + t.take(50).replace('\n', ' '))
    }

    private fun displayName(uri: Uri): String =
        runCatching {
            contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
        }.getOrNull() ?: uri.lastPathSegment ?: "file"

    private fun send(peer: Peer) {
        val fds = ArrayList<Int>()
        val names = ArrayList<String>()
        val texts = ArrayList<String>()
        for (s in staged) {
            if (s.text != null) { texts += s.text; continue }
            val fd = runCatching { contentResolver.openFileDescriptor(s.uri!!, "r")!!.detachFd() }
            fd.onSuccess { fds += it; names += s.label }.onFailure { toast("Skipped ${s.label}: ${it.message}") }
        }
        if (fds.isEmpty() && texts.isEmpty()) return
        Core.send(peer.id, fds.toIntArray(), names.toTypedArray(), texts.toTypedArray())
        staged.clear()
        renderStaged()
    }

    // ---------- rendering ----------

    private fun renderStaged() {
        stagedBox.removeAllViews()
        if (staged.isEmpty()) stagedBox.addView(hint("Tap Files, Paste, or use Share → Akhri Pasta in any app."))
        for (s in staged.toList()) {
            stagedBox.addView(row(text(s.label), button("x") { staged.remove(s); renderStaged() }))
        }
        render()
    }

    fun render() {
        val snap = snap
        offersBox.removeAllViews()
        for (o in snap.offers) {
            offersBox.addView(LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(12.dp, 12.dp, 12.dp, 12.dp)
                setBackgroundColor(Color.argb(40, 0x3d, 0x7e, 0xff))
                addView(text(o.summary).apply { setTypeface(typeface, Typeface.BOLD) })
                addView(row(button("Accept") { Core.decide(o.id, true) }, button("Decline") { Core.decide(o.id, false) }))
            })
        }
        devicesBox.removeAllViews()
        if (snap.peers.isEmpty()) devicesBox.addView(hint("Looking for devices on this Wi-Fi…"))
        for (p in snap.peers) {
            devicesBox.addView(Button(this).apply {
                text = "Send to ${p.name}"
                isAllCaps = false
                isEnabled = staged.isNotEmpty()
                contentDescription = "Send to ${p.name} at ${p.ip}"
                setOnClickListener { send(p) }
            })
        }
        jobsBox.removeAllViews()
        for (j in snap.jobs) {
            jobsBox.addView(text(j.label).apply { setTypeface(typeface, Typeface.BOLD) })
            if (j.active && j.fraction < 1f && j.status.contains('/')) {
                jobsBox.addView(ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal).apply {
                    max = 1000
                    progress = (j.fraction * 1000).toInt()
                })
            }
            val status = text(j.status).apply { if (j.failed) setTextColor(Color.rgb(0xd3, 0x2f, 0x2f)) }
            jobsBox.addView(if (j.active) row(status, button("Cancel") { Core.cancel(j.id) }) else status)
        }
    }

    private fun box(parent: LinearLayout) = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; parent.addView(this) }
    private fun text(s: String) = TextView(this).apply { text = s; textSize = 15f }
    private fun hint(s: String) = text(s).apply { alpha = 0.6f }
    private fun heading(s: String) = TextView(this).apply {
        text = s; textSize = 20f; setTypeface(typeface, Typeface.BOLD); setPadding(0, 16.dp, 0, 4.dp)
    }
    private fun button(s: String, onClick: () -> Unit) = Button(this).apply { text = s; isAllCaps = false; setOnClickListener { onClick() } }
    private fun row(first: View, vararg rest: View) = LinearLayout(this).apply {
        gravity = android.view.Gravity.CENTER_VERTICAL
        addView(first, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        rest.forEach { addView(it) }
    }
    private fun toast(s: String) = Toast.makeText(this, s, Toast.LENGTH_SHORT).show()

    companion object {
        private const val PICK = 1
        @Volatile var ui: MainActivity? = null
        @Volatile var snap = Snap(emptyArray())
        private var watching = false

        /** One background thread per process: waits on the engine, refreshes the UI,
         *  copies received text, and notifies about offers while the app isn't open. */
        fun startWatcher(app: Context) {
            synchronized(this) { if (watching) return; watching = true }
            val main = Handler(Looper.getMainLooper())
            val nm = app.getSystemService(NotificationManager::class.java)
            nm.createNotificationChannel(NotificationChannel("offers", "Incoming files", NotificationManager.IMPORTANCE_HIGH))
            var lastOffer = 0L
            thread(isDaemon = true, name = "akhri-watch") {
                var seen = 0L
                while (true) {
                    seen = Core.waitChange(seen, if (snap.jobs.any { it.active }) 250 else 30_000)
                    val s = Snap(Core.snapshot())
                    snap = s
                    main.post {
                        for (t in s.texts) {
                            app.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Akhri Pasta", t))
                            Toast.makeText(app, "Text received and copied", Toast.LENGTH_SHORT).show()
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
                .setSmallIcon(android.R.drawable.stat_sys_download)
                .setContentTitle("Incoming files")
                .setContentText(o.summary)
                .setContentIntent(open)
                .setAutoCancel(true)
                .build()
            nm.notify(o.id.toInt(), n)
        }
    }
}
