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
        private const val OLD_APP_ROOT = "/data/data/com.termux"
        private const val OLD_PREFIX = "$OLD_APP_ROOT/files/usr"
    }

    // The stock Termux root and DSports root must have equal length because
    // relocation replaces path strings inside already-compiled binaries.
    private val appRoot = File("/data/data/${context.packageName}")
    private val appFiles = File(appRoot, "files")
    val prefix = File(appFiles, "usr")
    val home = File(appFiles, "home")
    private val newAppRoot = appRoot.absolutePath
    private val newPrefix = prefix.absolutePath
    private val aptArchives = File(context.cacheDir, "apt/archives")
    val work = File(appFiles, "dsports-work")
    private val bootstrapZip = File(appFiles, "bootstrap-aarch64.zip")
    private val bootstrapMarker = File(appFiles, ".dsports-bootstrap-ready")
    private val rootRelocationMarker = File(appFiles, ".dsports-root-relocated-v2")
    private val packagesMarker = File(appFiles, ".dsports-compiler-packages-ready")
    private val linker = "/system/bin/linker64"
    private val shell = File(prefix, "bin/bash")
    private val termuxExec = File(prefix, "lib/libtermux-exec.so")

    fun ensureReady(log: (String) -> Unit) {
        if (OLD_APP_ROOT.length != newAppRoot.length || OLD_PREFIX.length != newPrefix.length) {
            throw IllegalStateException(
                "This DSports package ID changes the embedded runtime path length. " +
                    "Use a package ID with the same length as com.termux."
            )
        }
        appFiles.mkdirs()
        home.mkdirs()
        work.mkdirs()
        aptArchives.mkdirs()
        File(aptArchives, "partial").mkdirs()

        if (!bootstrapMarker.exists() || !shell.isFile || !termuxExec.exists()) {
            installBootstrap(log)
        }

        // Older test APKs only relocated files/usr. Repair an existing runtime
        // once so apt cache defaults and HOME/config paths no longer target
        // /data/data/com.termux after the app is updated.
        if (!rootRelocationMarker.isFile) {
            log("Checking existing runtime for leftover stock-Termux paths…")
            relocateAppRootReferences(prefix.toPath())
            rootRelocationMarker.writeText("relocated-root=" + newAppRoot + "\n")
        }

        // Repair the execute bits on every launch, including installations created
        // by earlier builds. The official bootstrap ZIP does not reliably preserve
        // executable mode bits for all Termux commands and APT transport helpers.
        repairExecutablePaths(log)

        if (!packagesMarker.exists()) {
            log("Updating embedded package indexes with DSports-private cache paths…")
            runAptCommand("update", log)
            log("Installing compiler, Python, Git, CMake, Ninja and LLVM inside DSports…")
            runAptCommand(
                "install -y python git cmake ninja clang llvm lld curl tar patch unzip pkg-config make",
                log
            )
            // Upstream .deb packages can contain additional stock-Termux path
            // strings in binaries and maintainer scripts. Relocate them before
            // checking tools or marking the environment ready.
            relocateAppRootReferences(prefix.toPath())
            repairExecutablePaths(log)
            log("Checking embedded compiler tools…")
            runShell(
                "python --version && git --version && cmake --version && ninja --version && clang --version && llvm-ar --version",
                home,
                log
            )
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
                environment()["TERMUX_APP__PACKAGE_NAME"] = context.packageName
                environment()["TERMUX_APP__DATA_DIR"] = appRoot.absolutePath
                environment()["TERMUX__ROOTFS"] = appFiles.absolutePath
                environment()["TERMUX__HOME"] = home.absolutePath
                environment()["TERMUX__PREFIX"] = prefix.absolutePath
                environment()["PATH"] = prefix.absolutePath + "/bin:" + prefix.absolutePath + "/bin/applets:/system/bin:/system/xbin"
                environment()["LD_LIBRARY_PATH"] = prefix.absolutePath + "/lib"
                environment()["LD_PRELOAD"] = termuxExec.absolutePath
                environment()["TMPDIR"] = File(context.cacheDir, "tmp").apply { mkdirs() }.absolutePath
                environment()["TMP"] = File(context.cacheDir, "tmp").absolutePath
                environment()["LANG"] = "en_US.UTF-8"
                environment()["ANDROID_ROOT"] = "/system"
                environment()["ANDROID_DATA"] = "/data"
            }
            .start()

        val recentOutput = mutableListOf<String>()
        BufferedInputStream(process.inputStream).bufferedReader().useLines { lines ->
            lines.forEach { line ->
                log(line)
                recentOutput.add(line)
                if (recentOutput.size > 30) recentOutput.removeAt(0)
            }
        }
        val exit = process.waitFor()
        if (exit != 0) {
            val tail = recentOutput.joinToString("\n")
            throw IllegalStateException(
                "Embedded command failed (exit " + exit + "): " + command.take(220) +
                    if (tail.isBlank()) "" else "\nLast command output:\n" + tail
            )
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
                        val target = zip.readBytes().toString(Charsets.UTF_8).replace(OLD_APP_ROOT, newAppRoot)
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
        relocateAppRootReferences(prefix.toPath())
        restoreSymlinksFromManifest(log)

        directoryModes.sortedByDescending { it.first.absolutePath.length }.forEach { (directory, mode) ->
            if (directory.isDirectory && mode != 0) {
                try { Os.chmod(directory.absolutePath, mode) } catch (_: Exception) {}
            }
        }

        repairExecutablePaths(log)

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
        bootstrapMarker.writeText("bootstrap-sha256=" + BOOTSTRAP_SHA256 + "\nrelocated-root=" + newAppRoot + "\nrelocated-prefix=" + newPrefix + "\n")
        rootRelocationMarker.writeText("relocated-root=" + newAppRoot + "\n")
        try { bootstrapZip.delete() } catch (_: Exception) {}
        log("Standalone shell ready. No separate terminal app is needed.")
    }

    /**
     * Termux's official bootstrap installer explicitly chmods these locations:
     * zip archives do not reliably retain executable mode bits across all
     * extractors. Missing x bits make apt return 126 before package installation.
     * Reapplying this narrowly-scoped policy is safe and repairs old app data too.
     */
    private fun repairExecutablePaths(log: (String) -> Unit) {
        val executableRoots = listOf(
            File(prefix, "bin"),
            File(prefix, "libexec"),
            File(prefix, "lib/apt/apt-helper"),
            File(prefix, "lib/apt/methods")
        )
        var repaired = 0
        for (root in executableRoots) {
            if (!root.exists()) continue
            val candidates = if (root.isDirectory) root.walkTopDown() else sequenceOf(root)
            for (file in candidates) {
                // chmod through links to the target, matching the official Termux
                // installer behaviour. Never traverse symlink directories.
                if (!file.isFile) continue
                try {
                    Os.chmod(file.absolutePath, 0x1C0) // 0700: owner read/write/execute
                    repaired++
                } catch (e: Exception) {
                    throw IllegalStateException(
                        "Could not make an embedded runtime tool executable: " + file.absolutePath, e
                    )
                }
            }
        }
        log("Verified executable permissions for " + repaired + " embedded runtime files.")
    }

    private fun runAptCommand(arguments: String, log: (String) -> Unit) {
        // Stock Termux apt normally stores downloaded archives under
        // /data/data/com.termux/cache. Pin its archive cache to DSports' own
        // private Android cache directory, including apt's required partial dir.
        aptArchives.mkdirs()
        File(aptArchives, "partial").mkdirs()
        val cacheOption = "-o Dir::Cache::archives=" + shellQuote(aptArchives.absolutePath)
        val command = "export DEBIAN_FRONTEND=noninteractive; apt-get " + cacheOption + " " + arguments
        try {
            runShell(command, home, log)
        } catch (first: Exception) {
            val details = first.message ?: ""
            if (!details.contains(OLD_APP_ROOT) &&
                !details.contains("Archives directory") &&
                !details.contains("com.termux/files/home/.termux")) {
                throw first
            }

            log("Detected a stock-Termux path in a package script. Repairing installed files and retrying configuration…")
            relocateAppRootReferences(prefix.toPath())
            repairExecutablePaths(log)
            aptArchives.mkdirs()
            File(aptArchives, "partial").mkdirs()

            // A failed apt run may have unpacked packages before a maintainer
            // script failed. Repair paths first, then finish configuration.
            try {
                runShell("export DEBIAN_FRONTEND=noninteractive; dpkg --configure -a", home, log)
            } catch (configure: Exception) {
                log("Package configuration pass reported: " + (configure.message ?: configure.javaClass.simpleName))
            }
            runShell(command, home, log)
        }
    }

    private fun shellQuote(value: String): String =
        "'" + value.replace("'", "'\\''") + "'"

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
            val target = line.substring(0, separator).replace(OLD_APP_ROOT, newAppRoot)
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

    /**
     * Rewrite the application data root, not just files/usr. Termux packages
     * bake /data/data/com.termux into apt's archive cache, HOME paths and
     * package maintainer scripts. Leaving those strings intact makes apt try
     * writing inside the separate Termux app's private sandbox.
     */
    private fun relocateAppRootReferences(root: Path) {
        val oldBytes = OLD_APP_ROOT.toByteArray(Charsets.UTF_8)
        val newBytes = newAppRoot.toByteArray(Charsets.UTF_8)
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
