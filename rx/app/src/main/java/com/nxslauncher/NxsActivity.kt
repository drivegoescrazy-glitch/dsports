package com.nxslauncher

import android.app.Activity
import android.app.ActivityOptions
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.hardware.display.DisplayManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.util.Log
import android.view.Display
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.WindowManager
import android.widget.Toast
import androidx.core.content.FileProvider
import org.json.JSONObject
import java.io.File
import java.net.URL

/**
 * Thin platform shell. Everything visual lives in Rust: this activity only owns
 * the surface, forwards input and performs the platform calls Rust cannot make
 * through JNI on its own (intent pickers, activity results).
 */
class NxsActivity : Activity(), SurfaceHolder.Callback {

    private lateinit var view: SurfaceView

    private external fun nativeCreate(surface: Surface, width: Int, height: Int, filesDir: String, packageName: String)
    private external fun nativeResize(width: Int, height: Int)
    private external fun nativeDestroy()
    private external fun nativeTouch(action: Int, x: Float, y: Float, downTime: Long, eventTime: Long)
    private external fun nativeKey(code: Int, down: Boolean)
    private external fun nativeBack()

    private var pendingPick: Int = 0
    private var updateThread: Thread? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.setFlags(
            WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
            WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS
        )
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        window.decorView.systemUiVisibility =
            View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or
                View.SYSTEM_UI_FLAG_FULLSCREEN or
                View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
                View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION

        installFonts()
        cleanupUpdates()

        view = SurfaceView(this)
        view.holder.addCallback(this)
        view.holder.setFormat(android.graphics.PixelFormat.OPAQUE)
        setContentView(view)
    }

    /**
     * CardsNX's Inter faces are bundled; they are unpacked once so the Rust
     * renderer can read them like any other file, keeping the APK's asset
     * compressor out of the hot path.
     */
    private fun installFonts() {
        val dir = File(filesDir, "fonts")
        dir.mkdirs()
        for (name in arrayOf("Inter-500.ttf", "Inter-600.ttf", "Inter-700.ttf")) {
            val out = File(dir, name)
            if (out.length() > 0L) continue
            try {
                assets.open("fonts/$name").use { input ->
                    out.outputStream().use { output -> input.copyTo(output) }
                }
            } catch (t: Throwable) {
                Log.e(TAG, "could not install $name", t)
            }
        }
    }

    // ---------------------------------------------------------------- surface

    override fun surfaceCreated(holder: SurfaceHolder) {
        val w = Math.max(1, view.width)
        val h = Math.max(1, view.height)
        // A throw from the native side must not take the launcher down: the next
        // surface event retries, and the reason lands in logcat.
        guard("create") {
            nativeCreate(holder.surface, w, h, filesDir.absolutePath, packageName)
        }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        guard("resize") { nativeResize(Math.max(1, width), Math.max(1, height)) }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        guard("destroy") { nativeDestroy() }
    }

    private inline fun guard(what: String, block: () -> Unit) {
        try {
            block()
        } catch (t: Throwable) {
            Log.e(TAG, "native $what failed", t)
        }
    }

    // ------------------------------------------------------------------ input

    override fun onTouchEvent(event: MotionEvent): Boolean {
        nativeTouch(
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> 0
                MotionEvent.ACTION_MOVE -> 2
                MotionEvent.ACTION_UP -> 1
                MotionEvent.ACTION_CANCEL -> 3
                else -> 2
            },
            event.x,
            event.y,
            event.downTime,
            event.eventTime
        )
        return true
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        nativeKey(keyCode, true)
        return true
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        nativeKey(keyCode, false)
        return true
    }

    override fun onBackPressed() {
        nativeBack()
    }

    override fun onResume() {
        super.onResume()
        view.requestFocus()
    }

    // ---------------------------------------------------------------- updates

    /**
     * Updates come from the public dsports repo: a tiny JSON manifest names the
     * newest version and where its APK lives. The download goes to the app's own
     * cache and every older APK is removed on the next start, so nothing is left
     * behind in shared storage — no permissions needed for either half.
     */
    @Suppress("unused")
    fun checkForUpdate() {
        if (updateThread?.isAlive == true) return
        val current = versionName()
        updateThread = Thread {
            try {
                val json = URL(UPDATE_MANIFEST).openConnection().run {
                    connectTimeout = 10_000
                    readTimeout = 20_000
                    getInputStream().bufferedReader().use { it.readText() }
                }
                val latest = JSONObject(json).optString("version")
                val url = JSONObject(json).optString("url")
                if (latest.isEmpty() || url.isEmpty() || compare(latest, current) <= 0) {
                    say("NXS Launcher is up to date ($current)")
                    return@Thread
                }
                val dir = File(cacheDir, "updates").apply { mkdirs() }
                val apk = File(dir, "NXSLauncher-$latest.apk")
                URL(url).openStream().use { input ->
                    apk.outputStream().use { output -> input.copyTo(output) }
                }
                say("Downloaded $latest, opening the installer")
                runOnUiThread { install(apk) }
            } catch (t: Throwable) {
                Log.e(TAG, "update check failed", t)
                say("Could not check for updates")
            }
        }.also { it.start() }
    }

    private fun install(apk: File) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !packageManager.canRequestPackageInstalls()) {
            toast("Allow NXS Launcher to install unknown apps, then try again")
            startActivity(
                Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName"))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            )
            return
        }
        val uri = FileProvider.getUriForFile(this, "$packageName.files", apk)
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        try {
            startActivity(intent)
        } catch (t: Throwable) {
            Log.e(TAG, "install failed", t)
            toast("Could not open the installer")
        }
    }

    /** Old downloads are useless once the installer has run; drop them at start. */
    private fun cleanupUpdates() {
        val dir = File(cacheDir, "updates")
        if (!dir.isDirectory) return
        val keep = "NXSLauncher-${versionName()}.apk"
        dir.listFiles()?.forEach { file -> if (file.name != keep) file.delete() }
    }

    private fun versionName(): String = try {
        packageManager.getPackageInfo(packageName, 0).versionName ?: "0"
    } catch (t: Throwable) {
        "0"
    }

    private fun compare(a: String, b: String): Int {
        val x = a.split('.').map { it.toIntOrNull() ?: 0 }
        val y = b.split('.').map { it.toIntOrNull() ?: 0 }
        for (i in 0 until maxOf(x.size, y.size)) {
            val d = (x.getOrElse(i) { 0 }) - (y.getOrElse(i) { 0 })
            if (d != 0) return d
        }
        return 0
    }

    private fun toast(text: String) {
        runOnUiThread { Toast.makeText(this, text, Toast.LENGTH_LONG).show() }
    }

    private fun say(text: String) {
        Log.i(TAG, text)
        toast(text)
    }

    // --------------------------------------------------- callbacks from Rust

    /** Called from Rust when the user asked for a custom image. */
    @Suppress("unused")
    fun pickImage(kind: Int) {
        pendingPick = kind
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "image/*"
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        startActivityForResult(intent, REQUEST_PICK)
    }

    /**
     * Called from Rust to launch an app, optionally on a specific physical
     * display. `slot` is -1 for Android's default choice, 0 for the primary
     * display and 1 for the first secondary (public) display.
     */
    @Suppress("unused")
    fun launchPackage(pkg: String, slot: Int): Boolean {
        val intent = packageManager.getLaunchIntentForPackage(pkg) ?: return false
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val target = displayForSlot(slot)
        val secondaryAllowed = packageManager.hasSystemFeature(
            PackageManager.FEATURE_ACTIVITIES_ON_SECONDARY_DISPLAYS
        )
        return try {
            if (slot >= 0 && target != Display.DEFAULT_DISPLAY && secondaryAllowed) {
                val options = ActivityOptions.makeBasic()
                options.setLaunchDisplayId(target)
                startActivity(intent, options.toBundle())
            } else {
                startActivity(intent)
            }
            true
        } catch (t: Throwable) {
            // A vendor may expose the second panel privately even though it is
            // listed. Falling back keeps the game launchable instead of dead.
            Log.e(TAG, "could not launch $pkg on display $target", t)
            try {
                startActivity(intent)
                true
            } catch (again: Throwable) {
                Log.e(TAG, "launch failed", again)
                false
            }
        }
    }

    private fun publicDisplays(): List<Display> {
        val manager = getSystemService(DisplayManager::class.java) ?: return emptyList()
        return manager.displays
            .filter { it.displayId == Display.DEFAULT_DISPLAY || (it.flags and Display.FLAG_PRIVATE) == 0 }
            .sortedBy { it.displayId }
    }

    private fun displayForSlot(slot: Int): Int {
        if (slot <= 0) return Display.DEFAULT_DISPLAY
        return publicDisplays()
            .filter { it.displayId != Display.DEFAULT_DISPLAY }
            .getOrNull(slot - 1)
            ?.displayId ?: Display.DEFAULT_DISPLAY
    }

    /** Number of launchable screens: the primary plus public secondaries. */
    @Suppress("unused")
    fun displayCount(): Int {
        return 1 + publicDisplays().count { it.displayId != Display.DEFAULT_DISPLAY }
    }

    /** Called from Rust when the PULSE panel opens or retunes. */
    @Suppress("unused")
    fun pulseProbe(): String = Pulse.probe()

    /** Applies a PULSE profile; returns OK or a message for the panel. */
    @Suppress("unused")
    fun pulseApply(json: String): String = Pulse.apply(json)

    /** Called from Rust to open the system UI where the default home is chosen. */
    @Suppress("unused")
    fun openHomeSettings() {
        startActivity(Intent(Settings.ACTION_HOME_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    @Deprecated("Platform callback for the document picker")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        //noinspection deprecation
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_PICK || resultCode != RESULT_OK) {
            pendingPick = 0
            return
        }
        val uri: Uri = data?.data ?: return
        val bitmap = decode(uri) ?: return
        val w = bitmap.width
        val h = bitmap.height
        val pixels = IntArray(w * h)
        bitmap.getPixels(pixels, 0, w, 0, 0, w, h)
        val rgba = ByteArray(w * h * 4)
        var i = 0
        for (p in pixels) {
            rgba[i] = ((p shr 16) and 0xFF).toByte()
            rgba[i + 1] = ((p shr 8) and 0xFF).toByte()
            rgba[i + 2] = (p and 0xFF).toByte()
            rgba[i + 3] = ((p ushr 24) and 0xFF).toByte()
            i += 4
        }
        bitmap.recycle()
        onImagePicked(pendingPick, w, h, rgba)
        pendingPick = 0
    }

    private external fun onImagePicked(kind: Int, width: Int, height: Int, rgba: ByteArray)

    private fun decode(uri: Uri): Bitmap? = try {
        contentResolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it) }
    } catch (t: Throwable) {
        null
    }

    companion object {
        private const val TAG = "NXS"
        private const val REQUEST_PICK = 4801

        /** Public manifest written by each release: {"version":..,"url":..}. */
        private const val UPDATE_MANIFEST =
            "https://raw.githubusercontent.com/drivegoescrazy-glitch/dsports/main/rx/docs/update.json"

        init {
            System.loadLibrary("nxs")
        }
    }
}
