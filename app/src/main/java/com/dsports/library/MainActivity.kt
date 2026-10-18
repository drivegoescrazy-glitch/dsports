package com.dsports.library

import android.app.Activity
import android.app.AlertDialog
import android.content.ContentUris
import android.content.ContentValues
import android.content.Intent
import android.media.MediaScannerConnection
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
import java.io.OutputStream

class MainActivity : Activity() {
    private val pickRomRequest = 410
    private val handler = Handler(Looper.getMainLooper())
    private lateinit var statusView: TextView
    private lateinit var romView: TextView
    private lateinit var buildButton: Button
    private lateinit var installButton: Button

    private var selectedRomUri: Uri? = null
    private var stagedRomUri: Uri? = null
    private var statusUri: Uri? = null
    private var builtApkUri: Uri? = null
    private var buildLaunched = false
    private var buildFinishedHandled = false

    private val sharedDir = File(
        Environment.getExternalStorageDirectory(),
        Environment.DIRECTORY_DOWNLOADS + "/DSports"
    )
    private val stagedRomPath get() = File(sharedDir, "soulsilver.nds").absolutePath
    private val builderPath get() = File(sharedDir, "dsports_termux_builder.py").absolutePath
    private val statusPath get() = File(sharedDir, "port-status.txt").absolutePath
    private val apkPath get() = File(sharedDir, "SoulSilver-ssrecomp.apk").absolutePath

    private val statusPoll = object : Runnable {
        override fun run() {
            val uri = statusUri
            if (uri != null && buildLaunched) {
                try {
                    val text = contentResolver.openInputStream(uri)?.bufferedReader()?.use { it.readText() }
                    if (!text.isNullOrBlank()) {
                        statusView.text = text
                        if (text.contains("BUILD_SUCCEEDED") && !buildFinishedHandled) {
                            buildFinishedHandled = true
                            buildLaunched = false
                            buildButton.isEnabled = true
                            stagedRomUri?.let {
                                try { contentResolver.delete(it, null, null) } catch (_: Exception) {}
                                stagedRomUri = null
                            }
                            MediaScannerConnection.scanFile(
                                this@MainActivity, arrayOf(apkPath),
                                arrayOf("application/vnd.android.package-archive")
                            ) { _, scannedUri ->
                                runOnUiThread {
                                    builtApkUri = scannedUri
                                    installButton.isEnabled = scannedUri != null
                                    statusView.text = statusView.text.toString() +
                                        if (scannedUri != null) "\n\nAPK ready. Tap Install ported game." else
                                            "\n\nBuild completed, but Android could not index the APK. Check Downloads/DSports."
                                }
                            }
                        } else if (text.contains("BUILD_FAILED") && !buildFinishedHandled) {
                            buildFinishedHandled = true
                            buildLaunched = false
                            buildButton.isEnabled = true
                            statusView.text = text + "\n\nBuild failed. The latest error is shown above."
                        }
                    }
                } catch (_: Exception) {
                    // The file may be mid-write while the local builder refreshes it.
                }
                if (buildLaunched) handler.postDelayed(this, 1200)
            }
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
            text = "NATIVE PORT BUILDER"
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
            text = "On-device compiler setup"
            setOnClickListener { showTermuxSetup() }
        })
        left.addView(TextView(this).apply {
            text = "Your ROM is processed locally. DSports does not upload it. The generated APK contains your game copy; keep it private."
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
            text = "Ready.\n\nSelect your own US SoulSilver .nds dump, then build. The first build downloads the Android NDK and SDL2 and may take a long time and several gigabytes of temporary storage."
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
        romView.text = "Selected:\n$name\n\nThe ROM is staged in Downloads/DSports for the local build. That staged copy is deleted after a successful build."
        buildButton.isEnabled = true
        statusView.text = "ROM selected: $name\n\nReady to create the APK locally."
    }

    private fun confirmAndBuild() {
        if (selectedRomUri == null) {
            chooseRom()
            return
        }
        if (Build.VERSION.SDK_INT < 29) {
            showMessage("Android version unsupported", "The on-device builder requires Android 10 or newer.")
            return
        }
        AlertDialog.Builder(this)
            .setTitle("Build on this device?")
            .setMessage(
                "DSports uses Termux as the local compiler environment, then runs the SSRecomp native port pipeline on your handheld. " +
                    "The first build downloads roughly 700 MB of Android NDK files plus SDL2 and needs additional free storage. " +
                    "Compilation may take 30–60 minutes or longer. The ROM is not uploaded; it is packaged into the final local APK."
            )
            .setPositiveButton("Start local build") { _, _ -> prepareAndLaunchBuilder() }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun prepareAndLaunchBuilder() {
        if (packageManager.getLaunchIntentForPackage("com.termux") == null) {
            showMessage(
                "Termux is required for the on-device compiler",
                "Install Termux, open it once, and run:\n\ntermux-setup-storage\npkg update -y\npkg install -y python git cmake ninja clang llvm curl tar\n\nThen complete the one-time integration steps under On-device compiler setup. The build and ROM processing stay on this device."
            )
            return
        }

        buildButton.isEnabled = false
        installButton.isEnabled = false
        statusView.text = "Preparing local files…\nThe ROM is not being uploaded."
        Thread {
            try {
                stagedRomUri = writeDownload("soulsilver.nds", "application/octet-stream") { output ->
                    val source = selectedRomUri ?: throw IllegalStateException("Select a ROM first.")
                    contentResolver.openInputStream(source)?.use { input -> input.copyTo(output, 1024 * 256) }
                        ?: throw IllegalStateException("Android could not open the selected ROM.")
                }
                writeDownload("dsports_termux_builder.py", "text/x-python") { output ->
                    assets.open("dsports_termux_builder.py").use { it.copyTo(output) }
                }
                statusUri = writeDownload("port-status.txt", "text/plain") { output ->
                    output.write("Waiting for Termux to start…\n".toByteArray(Charsets.UTF_8))
                }
                runOnUiThread { launchTermuxBuilder() }
            } catch (e: Exception) {
                runOnUiThread {
                    buildButton.isEnabled = true
                    statusView.text = "Could not prepare local files: " + (e.message ?: "unknown error")
                }
            }
        }.start()
    }

    private fun launchTermuxBuilder() {
        val commandPath = "/data/data/com.termux/files/usr/bin/bash"
        val workDir = "/data/data/com.termux/files/home"
        val command =
            "set -e; " +
                "pkg update -y && pkg install -y python git cmake ninja clang llvm curl tar && " +
                "mkdir -p \"\$HOME/.local/share/dsports\" && " +
                "cp " + escapeForShell(builderPath) + " \"\$HOME/.local/share/dsports/dsports_termux_builder.py\" && " +
                "exec python \"\$HOME/.local/share/dsports/dsports_termux_builder.py\" " +
                "--rom " + escapeForShell(stagedRomPath) + " " +
                "--status " + escapeForShell(statusPath) + " " +
                "--output " + escapeForShell(apkPath) + " --jobs 2"
        try {
            val intent = Intent("com.termux.RUN_COMMAND").apply {
                setClassName("com.termux", "com.termux.app.RunCommandService")
                putExtra("com.termux.RUN_COMMAND_PATH", commandPath)
                putExtra("com.termux.RUN_COMMAND_ARGUMENTS", arrayOf("-lc", command))
                putExtra("com.termux.RUN_COMMAND_WORKDIR", workDir)
                putExtra("com.termux.RUN_COMMAND_BACKGROUND", false)
                putExtra("com.termux.RUN_COMMAND_SESSION_ACTION", "0")
                putExtra("com.termux.RUN_COMMAND_COMMAND_LABEL", "Build SoulSilver native Android port")
                putExtra("com.termux.RUN_COMMAND_COMMAND_DESCRIPTION", "Compiles from the ROM selected locally. The ROM is not uploaded.")
            }
            startService(intent)
            buildLaunched = true
            buildFinishedHandled = false
            statusView.text = "Termux started. Waiting for the on-device compiler…"
            handler.removeCallbacks(statusPoll)
            handler.post(statusPoll)
        } catch (e: Exception) {
            buildButton.isEnabled = true
            statusView.text = "Termux did not accept the build command.\n\n" + (e.message ?: e.javaClass.simpleName)
            showTermuxSetup()
        }
    }

    private fun showTermuxSetup() {
        AlertDialog.Builder(this)
            .setTitle("One-time Termux setup")
            .setMessage(
                "The compiler runs locally through Termux; the ROM never leaves your device.\n\n" +
                    "1. Open Termux and run:\ntermux-setup-storage\n\n" +
                    "2. In Termux, enable external commands:\nmkdir -p ~/.termux && printf '\\nallow-external-apps = true\\n' >> ~/.termux/termux.properties\n\n" +
                    "3. Close and reopen Termux.\n" +
                    "4. In Android Settings → Apps → DSports → Permissions → Additional permissions, allow “Run commands in Termux environment”.\n" +
                    "5. Grant Termux storage access when asked, then return to DSports.\n\n" +
                    "DSports installs Python, Git, CMake, Ninja and LLVM through Termux when the build starts."
            )
            .setPositiveButton("Open Termux") { _, _ ->
                try { startActivity(packageManager.getLaunchIntentForPackage("com.termux")) } catch (_: Exception) {}
            }
            .setNegativeButton("Close", null)
            .show()
    }

    private fun installBuiltGame() {
        val uri = builtApkUri
        if (uri == null) {
            MediaScannerConnection.scanFile(this, arrayOf(apkPath), arrayOf("application/vnd.android.package-archive")) { _, scanned ->
                runOnUiThread {
                    builtApkUri = scanned
                    if (scanned == null) statusView.text = "Could not find the generated APK at:\n$apkPath"
                    else installBuiltGame()
                }
            }
            return
        }
        if (Build.VERSION.SDK_INT >= 26 && !packageManager.canRequestPackageInstalls()) {
            try {
                startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
            } catch (_: Exception) {
                showMessage("Allow APK installation", "Enable “Install unknown apps” for DSports in Android Settings, then tap Install ported game again.")
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
            ?: throw IllegalStateException("Android could not create Downloads/DSports/$name")
        try {
            contentResolver.openOutputStream(uri, "wt")?.use(writer)
                ?: throw IllegalStateException("Android could not write Downloads/DSports/$name")
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
