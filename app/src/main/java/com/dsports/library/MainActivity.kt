package com.dsports.library

import android.app.Activity
import android.app.AlertDialog
import android.content.ContentUris
import android.content.ContentValues
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.provider.Settings
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.io.File
import java.io.FileOutputStream
import java.io.OutputStream

class MainActivity : Activity() {
    private val pickRomRequest = 410
    private val handler = Handler(Looper.getMainLooper())
    private lateinit var statusView: TextView
    private lateinit var romView: TextView
    private lateinit var buildButton: Button
    private lateinit var installButton: Button
    private var selectedRomUri: Uri? = null
    private var builtApkUri: Uri? = null
    @Volatile private var buildRunning = false
    private var buildFinishedHandled = false

    private val toolchain by lazy { EmbeddedToolchain(applicationContext) }
    private val workDir get() = toolchain.work
    private val stagedRom get() = File(workDir, "soulsilver.nds")
    private val statusFile get() = File(workDir, "port-status.txt")
    private val outputApk get() = File(workDir, "SoulSilver-ssrecomp.apk")
    private val processLog get() = File(workDir, "runtime-output.log")

    private val statusPoll = object : Runnable {
        override fun run() {
            try {
                if (statusFile.isFile) {
                    val text = statusFile.readText()
                    if (text.isNotBlank()) statusView.text = text
                }
            } catch (_: Exception) {}
            if (buildRunning) handler.postDelayed(this, 900)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.decorView.systemUiVisibility = (
            View.SYSTEM_UI_FLAG_FULLSCREEN or
                View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
            )

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(20, 20, 20, 20)
            setBackgroundColor(0xFF10151D.toInt())
        }
        val left = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(16, 16, 16, 16)
            setBackgroundColor(0xFF192331.toInt())
        }
        left.addView(TextView(this).apply {
            text = "DSPORTS"
            textSize = 27f
            setTextColor(0xFFFFFFFF.toInt())
        })
        left.addView(TextView(this).apply {
            text = "STANDALONE NATIVE PORT BUILDER"
            textSize = 12f
            setTextColor(0xFF76D5C7.toInt())
            setPadding(0, 2, 0, 12)
        })
        romView = TextView(this).apply {
            text = "No SoulSilver ROM selected."
            textSize = 14f
            setTextColor(0xFFB8C7D9.toInt())
            setPadding(0, 8, 0, 12)
        }
        left.addView(romView)
        left.addView(Button(this).apply {
            text = "1 · Select SoulSilver ROM"
            setOnClickListener { chooseRom() }
        })
        buildButton = Button(this).apply {
            text = "2 · Build playable Android APK"
            isEnabled = false
            setOnClickListener { confirmAndBuild() }
        }
        left.addView(buildButton)
        installButton = Button(this).apply {
            text = "3 · Install ported game"
            isEnabled = false
            setOnClickListener { installBuiltGame() }
        }
        left.addView(installButton)
        left.addView(Button(this).apply {
            text = "Standalone builder details"
            setOnClickListener { showBuilderDetails() }
        })
        left.addView(TextView(this).apply {
            text = "The ROM stays in DSports private storage and is never uploaded. The generated APK contains your game copy; keep it private."
            textSize = 12f
            setTextColor(0xFF93A4B8.toInt())
            setPadding(0, 16, 0, 0)
        })

        val right = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(18, 12, 12, 12)
            setBackgroundColor(0xFF0D1117.toInt())
        }
        right.addView(TextView(this).apply {
            text = "LOCAL BUILD LOG"
            textSize = 20f
            setTextColor(0xFFFFFFFF.toInt())
            setPadding(0, 0, 0, 8)
        })
        statusView = TextView(this).apply {
            text = "Ready.\n\nSelect your own US SoulSilver .nds dump, then build. DSports installs its own private build tools; the separate Termux app is not required. The first build needs several gigabytes of free storage and can take a long time."
            textSize = 13f
            setTextColor(0xFFB8C7D9.toInt())
            setPadding(12, 12, 12, 12)
        }
        right.addView(ScrollView(this).apply {
            isFillViewport = true
            addView(statusView)
        }, LinearLayout.LayoutParams(-1, 0, 1f))
        root.addView(left, LinearLayout.LayoutParams(0, -1, 0.42f))
        root.addView(right, LinearLayout.LayoutParams(0, -1, 0.58f))
        setContentView(root)
    }

    private fun chooseRom() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        }
        startActivityForResult(intent, pickRomRequest)
    }

    @Deprecated("Legacy activity result API retained for broad Android compatibility")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != pickRomRequest || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        selectedRomUri = uri
        try {
            contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        } catch (_: Exception) {}
        val name = queryDisplayName(uri) ?: "selected .nds file"
        romView.text = "Selected:\n" + name + "\n\nThe ROM will be copied into DSports private storage for the local build."
        buildButton.isEnabled = true
        statusView.text = "ROM selected: " + name + "\n\nReady to create the APK locally."
    }

    private fun confirmAndBuild() {
        if (buildRunning) return
        if (selectedRomUri == null) {
            chooseRom()
            return
        }
        if (!Build.SUPPORTED_ABIS.any { it == "arm64-v8a" }) {
            showMessage("Device not supported", "The current embedded compiler backend is configured for 64-bit ARM Android devices.")
            return
        }
        AlertDialog.Builder(this)
            .setTitle("Build on this device?")
            .setMessage(
                "DSports no longer requires the separate Termux app. On first build it downloads a checksum-verified standalone shell environment, installs Python, Git, Clang/LLVM, CMake and Ninja inside DSports, then downloads the Android NDK and SDL2 needed by the recompiler. Several gigabytes of free storage are recommended. The ROM is kept private and is not uploaded."
            )
            .setPositiveButton("Start local build") { _, _ -> startEmbeddedBuild() }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun startEmbeddedBuild() {
        if (buildRunning) return
        buildRunning = true
        buildFinishedHandled = false
        buildButton.isEnabled = false
        installButton.isEnabled = false
        workDir.mkdirs()
        statusFile.writeText("Preparing DSports standalone build environment…\n")
        handler.removeCallbacks(statusPoll)
        handler.post(statusPoll)

        Thread {
            try {
                val romUri = selectedRomUri ?: throw IllegalStateException("Select a ROM first.")
                FileOutputStream(stagedRom).use { output ->
                    contentResolver.openInputStream(romUri)?.use { input ->
                        input.copyTo(output, 256 * 1024)
                    } ?: throw IllegalStateException("Android could not read the selected ROM.")
                }
                if (stagedRom.length() < 1024L * 1024L) {
                    throw IllegalStateException("The selected file is too small to be a Nintendo DS ROM.")
                }
                writeStatus("ROM copied to DSports private storage. Preparing the embedded compiler…")
                toolchain.ensureReady { line -> appendRuntimeLog(line) }

                val scriptDir = File(toolchain.home, ".local/share/dsports")
                scriptDir.mkdirs()
                val script = File(scriptDir, "dsports_embedded_builder.py")
                assets.open("dsports_embedded_builder.py").use { input ->
                    FileOutputStream(script).use { output -> input.copyTo(output) }
                }
                outputApk.delete()
                processLog.delete()
                writeStatus("Standalone compiler is ready. Generating native C++ and building the Android game APK…")
                val cmd = "exec python " + escapeForShell(script.absolutePath) +
                    " --rom " + escapeForShell(stagedRom.absolutePath) +
                    " --status " + escapeForShell(statusFile.absolutePath) +
                    " --output " + escapeForShell(outputApk.absolutePath) + " --jobs 2"
                toolchain.runShell(cmd, toolchain.home) { line -> appendRuntimeLog(line) }

                val resultText = if (statusFile.isFile) statusFile.readText() else ""
                if (!resultText.contains("BUILD_SUCCEEDED") || !outputApk.isFile || outputApk.length() < 100_000L) {
                    throw IllegalStateException("The builder exited without producing a verified APK. Check the build log for the last error.")
                }

                val savedUri = writeDownload(
                    "SoulSilver-ssrecomp.apk",
                    "application/vnd.android.package-archive"
                ) { output ->
                    outputApk.inputStream().use { input -> input.copyTo(output, 256 * 1024) }
                }
                runOnUiThread {
                    builtApkUri = savedUri
                    buildFinishedHandled = true
                    buildRunning = false
                    buildButton.isEnabled = true
                    installButton.isEnabled = true
                    statusView.text = resultText + "\n\nAPK copied to Downloads/DSports. Tap Install ported game."
                }
            } catch (e: Exception) {
                val message = e.message ?: e.javaClass.simpleName
                appendRuntimeLog("ERROR: " + message)
                writeStatus("BUILD_FAILED\n" + message + "\n\nThe last error is also saved in DSports private storage.")
                runOnUiThread {
                    buildFinishedHandled = true
                    buildRunning = false
                    buildButton.isEnabled = true
                    installButton.isEnabled = builtApkUri != null
                    statusView.text = statusFile.takeIf { it.isFile }?.readText()
                        ?: ("Build failed: " + message)
                }
            }
        }.start()
    }

    private fun writeStatus(message: String) {
        try { statusFile.parentFile?.mkdirs(); statusFile.writeText(message + "\n") } catch (_: Exception) {}
    }

    @Synchronized
    private fun appendRuntimeLog(line: String) {
        try {
            processLog.parentFile?.mkdirs()
            processLog.appendText(line + "\n")
        } catch (_: Exception) {}
    }

    private fun showBuilderDetails() {
        AlertDialog.Builder(this)
            .setTitle("Standalone build environment")
            .setMessage(
                "DSports uses a private build environment inside its own app storage; it does not query, launch, or require the separate Termux app. The first build downloads a verified bootstrap archive and then installs compiler packages locally. The SoulSilver ROM and generated game files never go to a server.\n\nThe native recompiler/toolchain is experimental. A successful Android app build does not by itself prove the SoulSilver port compiles or runs on every device."
            )
            .setPositiveButton("OK", null)
            .show()
    }

    private fun installBuiltGame() {
        val uri = builtApkUri
        if (uri == null) {
            showMessage("APK not ready", "Build the port successfully first.")
            return
        }
        if (Build.VERSION.SDK_INT >= 26 && !packageManager.canRequestPackageInstalls()) {
            try {
                startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + packageName)))
            } catch (_: Exception) {
                showMessage("Allow APK installation", "Enable Install unknown apps for DSports in Android Settings, then tap Install ported game again.")
            }
            return
        }
        try {
            startActivity(Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, "application/vnd.android.package-archive")
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            })
        } catch (e: Exception) {
            showMessage("Could not open installer", e.message ?: "Android could not open the generated APK.")
        }
    }

    private fun writeDownload(name: String, mime: String, writer: (OutputStream) -> Unit): Uri {
        val collection = MediaStore.Downloads.EXTERNAL_CONTENT_URI
        findDownload(name)?.let { try { contentResolver.delete(it, null, null) } catch (_: Exception) {} }
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, name)
            put(MediaStore.MediaColumns.MIME_TYPE, mime)
            put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/DSports")
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val uri = contentResolver.insert(collection, values)
            ?: throw IllegalStateException("Android could not create Downloads/DSports/" + name)
        try {
            contentResolver.openOutputStream(uri, "wt")?.use(writer)
                ?: throw IllegalStateException("Android could not write Downloads/DSports/" + name)
            contentResolver.update(uri, ContentValues().apply {
                put(MediaStore.MediaColumns.IS_PENDING, 0)
            }, null, null)
            return uri
        } catch (e: Exception) {
            try { contentResolver.delete(uri, null, null) } catch (_: Exception) {}
            throw e
        }
    }

    private fun findDownload(name: String): Uri? {
        val cursor = contentResolver.query(
            MediaStore.Downloads.EXTERNAL_CONTENT_URI,
            arrayOf(MediaStore.MediaColumns._ID),
            MediaStore.MediaColumns.DISPLAY_NAME + "=?",
            arrayOf(name),
            MediaStore.MediaColumns.DATE_MODIFIED + " DESC"
        ) ?: return null
        cursor.use {
            if (!it.moveToFirst()) return null
            return ContentUris.withAppendedId(MediaStore.Downloads.EXTERNAL_CONTENT_URI, it.getLong(0))
        }
    }

    private fun queryDisplayName(uri: Uri): String? {
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) return it.getString(0)
        }
        return uri.lastPathSegment?.substringAfterLast('/')
    }

    private fun escapeForShell(value: String): String = "'" + value.replace("'", "'\\''") + "'"

    private fun showMessage(title: String, message: String) {
        AlertDialog.Builder(this).setTitle(title).setMessage(message).setPositiveButton("OK", null).show()
    }

    override fun onDestroy() {
        handler.removeCallbacks(statusPoll)
        super.onDestroy()
    }
}
