package com.nxslauncher

import android.app.Activity
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.WindowManager

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

        view = SurfaceView(this)
        view.holder.addCallback(this)
        view.holder.setFormat(android.graphics.PixelFormat.OPAQUE)
        setContentView(view)
    }

    // ---------------------------------------------------------------- surface

    override fun surfaceCreated(holder: SurfaceHolder) {
        val w = Math.max(1, view.width)
        val h = Math.max(1, view.height)
        nativeCreate(holder.surface, w, h, filesDir.absolutePath, packageName)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        nativeResize(Math.max(1, width), Math.max(1, height))
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        nativeDestroy()
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

    /** Called from Rust to launch an app by package name. */
    @Suppress("unused")
    fun launchPackage(pkg: String): Boolean {
        val intent = packageManager.getLaunchIntentForPackage(pkg) ?: return false
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        return try {
            startActivity(intent)
            true
        } catch (t: Throwable) {
            false
        }
    }

    /** Called from Rust to open the system UI where the default home is chosen. */
    @Suppress("unused")
    fun openHomeSettings() {
        startActivity(Intent(Settings.ACTION_HOME_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    @Deprecated("Platform callback for the document picker")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
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
        private const val REQUEST_PICK = 4801

        init {
            System.loadLibrary("nxs")
        }
    }
}
