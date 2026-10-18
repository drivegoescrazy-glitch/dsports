package com.dsports.library

import android.content.Context
import android.system.Os
import org.apache.commons.compress.archivers.zip.ZipArchiveInputStream
import java.io.BufferedInputStream
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.nio.file.FileVisitResult
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.SimpleFileVisitor
import java.nio.file.attribute.BasicFileAttributes
import java.security.MessageDigest
import java.util.Locale

/**
 * Private in-app build environment. It downloads a checksum-pinned official bootstrap,
 * relocates the compiled prefix to DSports' private directory, and launches bundled
 * tools through Android's system linker. The Termux app itself is never used.
 */
class EmbeddedToolchain(private val context: Context) {
    companion object {
        private const val BOOTSTRAP_URL =
            "https://github.com/termux/termux-packages/releases/download/bootstrap-2026.10.18-r1%2Bapt.android-7/bootstrap-aarch64.zip"
        private const val BOOTSTRAP_SHA256 =
            "7b170ff5ef0befe501d01febc61382f3c6e34456521853cff9ba0b277aa52fe9"
        private const val OLD_PREFIX = "/data/data/com.termux/files/usr"
        private const val NEW_PREFIX = "/data/data/com.dsprtf/files/usr"
    }

    private val appFiles = File("/data/data/com.dsprtf/files")
    val prefix = File(appFiles, "usr")
    val home = File(appFiles, "home")
    val work = File(appFiles, "dsports-work")
    private val bootstrapZip = File(appFiles, "bootstrap-aarch64.zip")
    private val bootstrapMarker = File(appFiles, ".dsports-bootstrap-ready")
    private val packagesMarker = File(appFiles, ".dsports-compiler-packages-ready")
    private val linker = "/system/bin/linker64"
    private val shell = File(prefix, "bin/bash")
    private val termuxExec = File(prefix, "lib/libtermux-exec.so")

    fun ensureReady(log: (String) -> Unit) {
        if (OLD_PREFIX.length != NEW_PREFIX.length) {
            throw IllegalStateException("Embedded runtime prefix relocation must preserve binary string lengths.")
        }
        appFiles.mkdirs()
        home.mkdirs()
        work.mkdirs()

        if (!bootstrapMarker.exists() || !shell.isFile || !termuxExec.exists()) {
            installBootstrap(log)
        }

        if (!packagesMarker.exists()) {
            log("Installing compiler, Python, Git, CMake, Ninja and LLVM inside DSports…")
            val command = """
                set -e
                export DEBIAN_FRONTEND=noninteractive
                apt update -y
                apt install -y python git cmake ninja clang llvm lld curl tar patch unzip pkg-config make
                python --version
                git --version
                cmake --version
                ninja --version
                clang --version
                llvm-ar --version
            """.trimIndent()
            runShell(command, home, log)
            packagesMarker.writeText("installed\n")
        }
    }

    fun runShell(command: String, workingDirectory: File = home, log: (String) -> Unit) {
        val process = ProcessBuilder(linker, shell.absolutePath, "-lc", command)
            .directory(workingDirectory)
            .redirectErrorStream(true)
            .apply {
                environment()["HOME"] = home.absolutePath
                environment()["PREFIX"] = prefix.absolutePath
                environment()["TERMUX_PREFIX"] = prefix.absolutePath
                environment()["TERMUX_APP__PACKAGE_NAME"] = "com.dsprtf"
                environment()["PATH"] = prefix.absolutePath + "/bin"
                environment()["LD_LIBRARY_PATH"] = prefix.absolutePath + "/lib"
                environment()["LD_PRELOAD"] = termuxExec.absolutePath
                environment()["TMPDIR"] = File(context.cacheDir, "tmp").apply { mkdirs() }.absolutePath
                environment()["TMP"] = File(context.cacheDir, "tmp").absolutePath
                environment()["LANG"] = "en_US.UTF-8"
                environment()["ANDROID_ROOT"] = "/system"
                environment()["ANDROID_DATA"] = "/data"
            }
            .start()

        BufferedInputStream(process.inputStream).bufferedReader().useLines { lines ->
            lines.forEach { line -> log(line) }
        }
        val exit = process.waitFor()
        if (exit != 0) {
            throw IllegalStateException("Embedded command failed (exit " + exit + "): " + command.take(220))
        }
    }

    private fun installBootstrap(log: (String) -> Unit) {
        log("Downloading the verified standalone build environment (about 33 MB)…")
        downloadBootstrap(log)
        log("Unpacking and relocating the build environment into DSports private storage…")
        if (prefix.exists()) prefix.deleteRecursively()
        prefix.mkdirs()
        val directoryModes = mutableListOf<Pair<File, Int>>()

        ZipArchiveInputStream(
            BufferedInputStream(bootstrapZip.inputStream()),
            "UTF-8",
            true,
            true
        ).use { zip ->
            while (true) {
                val entry = zip.nextZipEntry ?: break
                val name = entry.name.replace('\\', '/').trimStart('/')
                if (name.isBlank() || name == ".") continue
                val destination = File(prefix, name)
                val normalizedRoot = prefix.toPath().toAbsolutePath().normalize()
                val normalizedDestination = destination.toPath().toAbsolutePath().normalize()
                if (!normalizedDestination.startsWith(normalizedRoot)) {
                    throw IllegalStateException("Unsafe path in embedded bootstrap: " + name)
                }

                val mode = entry.unixMode
                when {
                    (mode and 0xF000) == 0xA000 -> {
                        destination.parentFile?.mkdirs()
                        val target = zip.readBytes().toString(Charsets.UTF_8).replace(OLD_PREFIX, NEW_PREFIX)
                        try {
                            Os.symlink(target, destination.absolutePath)
                        } catch (e: Exception) {
                            if (!Files.isSymbolicLink(destination.toPath())) throw e
                        }
                    }
                    entry.isDirectory || (mode and 0xF000) == 0x4000 -> {
                        destination.mkdirs()
                        directoryModes.add(destination to (mode and 0x1FF))
                    }
                    else -> {
                        destination.parentFile?.mkdirs()
                        FileOutputStream(destination).use { output -> zip.copyTo(output) }
                        val perms = mode and 0x1FF
                        try { Os.chmod(destination.absolutePath, if (perms == 0) 0x1A4 else perms) }
                        catch (_: Exception) {}
                    }
                }
            }
        }

        // Termux bootstrap ZIPs intentionally store symlink definitions in
        // SYMLINKS.txt instead of storing Unix symlinks in the ZIP itself.
        // Restore absolute-prefix references before recreating those links.
        relocatePrefixReferences(prefix.toPath())
        restoreSymlinksFromManifest(log)

        directoryModes.sortedByDescending { it.first.absolutePath.length }.forEach { (directory, mode) ->
            if (directory.isDirectory && mode != 0) {
                try { Os.chmod(directory.absolutePath, mode) } catch (_: Exception) {}
            }
        }

        if (!shell.isFile) {
            throw IllegalStateException(
                "The embedded bootstrap unpacked, but usr/bin/bash was not found at " +
                    shell.absolutePath + ". The bootstrap archive layout may have changed."
            )
        }
        if (!termuxExec.isFile) {
            throw IllegalStateException(
                "The embedded bootstrap unpacked, but libtermux-exec.so was not found at " +
                    termuxExec.absolutePath + ". Check the bootstrap symlink manifest."
            )
        }
        bootstrapMarker.writeText("bootstrap-sha256=" + BOOTSTRAP_SHA256 + "\nrelocated-prefix=" + NEW_PREFIX + "\n")
        try { bootstrapZip.delete() } catch (_: Exception) {}
        log("Standalone shell ready. No separate terminal app is needed.")
    }

    private fun downloadBootstrap(log: (String) -> Unit) {
        if (bootstrapZip.exists() && sha256(bootstrapZip) == BOOTSTRAP_SHA256) return
        try { bootstrapZip.delete() } catch (_: Exception) {}
        val connection = (URL(BOOTSTRAP_URL).openConnection() as HttpURLConnection).apply {
            connectTimeout = 20_000
            readTimeout = 60_000
            instanceFollowRedirects = true
            requestMethod = "GET"
        }
        try {
            connection.connect()
            if (connection.responseCode !in 200..299) {
                throw IllegalStateException("Bootstrap download failed with HTTP " + connection.responseCode + ".")
            }
            val total = connection.contentLengthLong
            var copied = 0L
            var lastReported = 0L
            connection.inputStream.use { input ->
                FileOutputStream(bootstrapZip).use { output ->
                    val buffer = ByteArray(256 * 1024)
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        output.write(buffer, 0, count)
                        copied += count
                        if (copied - lastReported >= 1024 * 1024) {
                            val percent = if (total > 0L) " (" + (copied * 100 / total).toInt() + "%)" else ""
                            log("Downloading standalone runtime: " + (copied / (1024 * 1024)) + " MB" + percent)
                            lastReported = copied
                        }
                    }
                }
            }
        } finally {
            connection.disconnect()
        }
        val actual = sha256(bootstrapZip)
        if (!actual.equals(BOOTSTRAP_SHA256, ignoreCase = true)) {
            bootstrapZip.delete()
            throw IllegalStateException("Standalone runtime integrity check failed; the downloaded file was deleted.")
        }
    }

    private fun sha256(file: File): String {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(256 * 1024)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                digest.update(buffer, 0, count)
            }
        }
        return digest.digest().joinToString("") { "%02x".format(Locale.ROOT, it) }
    }

    /**
     * Restore the symlinks encoded by Termux's bootstrap generator.
     *
     * The official bootstrap creator removes Unix symlinks before zipping the
     * prefix and records each one as "target←./path" in SYMLINKS.txt. Merely
     * unzipping the archive therefore leaves tools such as sh, pkg-config and
     * parts of the dynamic runtime missing even though extraction appears to
     * have succeeded.
     */
    private fun restoreSymlinksFromManifest(log: (String) -> Unit) {
        val manifest = File(prefix, "SYMLINKS.txt")
        if (!manifest.isFile) {
            throw IllegalStateException(
                "The bootstrap has no SYMLINKS.txt manifest. Cannot safely initialize the embedded runtime."
            )
        }

        val normalizedRoot = prefix.toPath().toAbsolutePath().normalize()
        var restored = 0
        var seen = 0
        manifest.forEachLine(Charsets.UTF_8) { line ->
            val separator = line.lastIndexOf('←')
            if (separator <= 0 || separator >= line.lastIndex) return@forEachLine
            seen++
            val target = line.substring(0, separator).replace(OLD_PREFIX, NEW_PREFIX)
            val relativeLink = line.substring(separator + 1)
                .replace('\\', '/')
                .removePrefix("./")
                .trimStart('/')
            if (relativeLink.isBlank()) return@forEachLine

            val destination = File(prefix, relativeLink)
            val normalizedDestination = destination.toPath().toAbsolutePath().normalize()
            if (!normalizedDestination.startsWith(normalizedRoot)) {
                throw IllegalStateException("Unsafe symlink path in bootstrap manifest: " + relativeLink)
            }

            destination.parentFile?.mkdirs()
            // The manifest is authoritative: a stored placeholder at this path
            // must not prevent the symlink from being recreated.
            java.nio.file.Files.deleteIfExists(normalizedDestination)
            try {
                Os.symlink(target, destination.absolutePath)
                restored++
            } catch (e: Exception) {
                throw IllegalStateException(
                    "Could not restore embedded-runtime symlink " + relativeLink + " -> " + target, e
                )
            }
        }

        if (seen == 0 || restored == 0) {
            throw IllegalStateException(
                "The bootstrap symlink manifest contained no usable entries; the runtime is incomplete."
            )
        }
        log("Restored " + restored + " embedded runtime symlinks.")
    }

    private fun relocatePrefixReferences(root: Path) {
        val oldBytes = OLD_PREFIX.toByteArray(Charsets.UTF_8)
        val newBytes = NEW_PREFIX.toByteArray(Charsets.UTF_8)
        Files.walkFileTree(root, object : SimpleFileVisitor<Path>() {
            override fun visitFile(file: Path, attrs: BasicFileAttributes): FileVisitResult {
                if (!attrs.isRegularFile || attrs.isSymbolicLink) return FileVisitResult.CONTINUE
                val bytes = try { Files.readAllBytes(file) } catch (_: Exception) {
                    return FileVisitResult.CONTINUE
                }
                var changed = false
                var i = 0
                while (i <= bytes.size - oldBytes.size) {
                    var matches = true
                    for (j in oldBytes.indices) {
                        if (bytes[i + j] != oldBytes[j]) {
                            matches = false
                            break
                        }
                    }
                    if (matches) {
                        System.arraycopy(newBytes, 0, bytes, i, newBytes.size)
                        changed = true
                        i += oldBytes.size
                    } else {
                        i++
                    }
                }
                if (changed) {
                    try { Files.write(file, bytes) }
                    catch (e: Exception) { throw IllegalStateException("Could not relocate embedded runtime file: " + file, e) }
                }
                return FileVisitResult.CONTINUE
            }
        })
    }
}
