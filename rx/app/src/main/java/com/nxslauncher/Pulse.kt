package com.nxslauncher

import android.os.Build
import android.os.IBinder
import android.os.Parcel
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * PULSE underclocker backend.
 *
 * Discovery reads the cpufreq policies and the Adreno power-level table from
 * sysfs. Applying limits goes through the device's own PServer service (the
 * no-root technique used by ClusterTune and PULSE), so the launcher never asks
 * for root and degrades to a clear "not supported" state on other hardware.
 *
 * The Rust UI owns profiles and the control loop; this object only reports the
 * hardware and executes one script at a time.
 */
object Pulse {

    data class CpuPolicy(
        val id: Int,
        val label: String,
        val minKhz: Int,
        val maxKhz: Int,
        val currentKhz: Int,
        val currentMinKhz: Int,
        val freqs: List<Int>,
        val minPath: String,
        val maxPath: String,
        val curPath: String,
    )

    data class GpuPolicy(
        val freqs: List<Int>,
        val minKhz: Int,
        val maxKhz: Int,
        val currentKhz: Int,
        val minPwrPath: String,
        val maxPwrPath: String,
        val curPath: String,
    )

    private var binder: IBinder? = null
    private var binderTried = false
    private var cpuPolicies: List<CpuPolicy> = emptyList()
    private var gpuPolicy: GpuPolicy? = null
    private var error: String = ""
    private var cpuTempPath = ""
    private var batteryTempPath = ""
    private var batteryCapacityPath = ""

    @Synchronized
    fun probe(): String {
        detectIfNeeded()
        val root = JSONObject()
        root.put("available", binder != null && cpuPolicies.isNotEmpty())
        root.put("error", error)
        root.put("device", Build.MODEL ?: "")
        root.put("socModel", socModel())
        val clusters = JSONArray()
        cpuPolicies.forEach { policy ->
            val item = JSONObject()
            item.put("id", policy.id)
            item.put("label", policy.label)
            item.put("min", policy.minKhz)
            item.put("max", policy.maxKhz)
            item.put("current", policy.currentKhz)
            item.put("curPath", policy.curPath)
            item.put("maxPath", policy.maxPath)
            item.put("minPath", policy.minPath)
            val freqs = JSONArray()
            policy.freqs.forEach { freqs.put(it) }
            item.put("freqs", freqs)
            clusters.put(item)
        }
        root.put("clusters", clusters)
        gpuPolicy?.let { gpu ->
            val item = JSONObject()
            item.put("minKhz", gpu.minKhz)
            item.put("maxKhz", gpu.maxKhz)
            item.put("currentKhz", gpu.currentKhz)
            item.put("curPath", gpu.curPath)
            val levels = JSONArray()
            gpu.freqs.forEach { levels.put(it) }
            item.put("levels", levels)
            root.put("gpu", item)
        }
        root.put("cpuTemp", cpuTempPath)
        root.put("batteryTemp", batteryTempPath)
        root.put("batteryCapacity", batteryCapacityPath)
        return root.toString()
    }

    /**
     * Applies one profile. Returns "OK" or an "ERR: ..." string the panel shows
     * verbatim; the UI never claims success without this reply.
     */
    @Synchronized
    fun apply(json: String): String {
        detectIfNeeded()
        if (binder == null) {
            return "ERR: This device does not expose the PServer service PULSE needs."
        }
        if (cpuPolicies.isEmpty()) {
            return "ERR: Could not read this device's CPU frequency policies."
        }
        val request = try {
            JSONObject(json)
        } catch (t: Throwable) {
            return "ERR: Malformed PULSE profile."
        }
        val restore = request.optBoolean("restore", false)
        val caps = mutableMapOf<Int, Int>()
        request.optJSONObject("clusters")?.let { object_ ->
            cpuPolicies.forEach { policy ->
                val value = object_.optInt(policy.id.toString(), 0)
                if (value > 0) caps[policy.id] = value
            }
        }
        val targets = cpuPolicies.map {
            PulseCommandBuilder.Target(
                id = it.id,
                freqs = it.freqs,
                minKhz = it.minKhz,
                maxKhz = it.maxKhz,
                currentMinKhz = it.currentMinKhz,
                minPath = it.minPath,
                maxPath = it.maxPath,
            )
        }
        val gpu = gpuPolicy?.let {
            PulseCommandBuilder.Gpu(
                freqs = it.freqs,
                minPwrPath = it.minPwrPath,
                maxPwrPath = it.maxPwrPath,
            )
        }
        val script = PulseCommandBuilder.build(
            restore = restore,
            caps = caps,
            targets = targets,
            gpu = gpu,
            gpuKhz = request.optInt("gpuKhz", 0),
        )
        return try {
            exec(script)
            "OK"
        } catch (t: Throwable) {
            "ERR: " + (t.message ?: "PServer rejected the profile")
        }
    }

    // ------------------------------------------------------------- discovery

    private fun detectIfNeeded() {
        if (binderTried) return
        binderTried = true
        binder = runCatching {
            val manager = Class.forName("android.os.ServiceManager")
            val getService = manager.getDeclaredMethod("getService", String::class.java)
            getService.invoke(null, "PServerBinder") as? IBinder
        }.getOrNull()
        if (binder == null) {
            error = "This device does not expose the PServer service PULSE needs. " +
                "AYN and Retroid handhelds on stock firmware provide it."
            return
        }
        cpuPolicies = detectCpuPolicies()
        gpuPolicy = detectGpu()
        cpuTempPath = findThermalZone()
        batteryTempPath = firstReadable(
            "/sys/class/power_supply/battery/temp",
            "/sys/class/power_supply/bms/temp",
        )
        batteryCapacityPath = firstReadable(
            "/sys/class/power_supply/battery/capacity",
            "/sys/class/power_supply/bms/capacity",
        )
        if (cpuPolicies.isEmpty()) {
            error = "PServer is present but no CPU frequency policies could be read. " +
                "This firmware exposes a different layout."
        } else {
            error = ""
        }
    }

    private fun detectCpuPolicies(): List<CpuPolicy> {
        val root = "/sys/devices/system/cpu/cpufreq"
        val dirs = listDir(root)
            .filter { it.startsWith("policy") }
            .sortedBy { it.removePrefix("policy").toIntOrNull() ?: Int.MAX_VALUE }
        val out = mutableListOf<CpuPolicy>()
        dirs.forEachIndexed { index, name ->
            val id = name.removePrefix("policy").toIntOrNull() ?: return@forEachIndexed
            val path = "$root/$name"
            val maxPath = "$path/scaling_max_freq"
            val minPath = "$path/scaling_min_freq"
            val curPath = "$path/scaling_cur_freq"
            val max = readInt(maxPath) ?: 0
            val min = readInt(minPath) ?: 0
            val hardwareMax = readInt("$path/cpuinfo_max_freq") ?: max
            var freqs = parseFreqs(readText("$path/scaling_available_frequencies"))
            if (freqs.isEmpty()) {
                freqs = listOf(min, max, hardwareMax).filter { it > 0 }.distinct().sorted()
            }
            if (min <= 0 && freqs.isEmpty()) return@forEachIndexed
            val selectableMax = freqs.lastOrNull() ?: hardwareMax
            val cores = parseIds(readText("$path/affected_cpus"))
                .ifEmpty { parseIds(readText("$path/related_cpus")) }
                .size
                .coerceAtLeast(1)
            out += CpuPolicy(
                id = id,
                label = "Cluster $index · $cores ${if (cores == 1) "core" else "cores"}",
                minKhz = freqs.firstOrNull() ?: min,
                maxKhz = selectableMax,
                currentKhz = max.takeIf { it > 0 } ?: selectableMax,
                currentMinKhz = min,
                freqs = freqs,
                minPath = minPath,
                maxPath = maxPath,
                curPath = curPath,
            )
        }
        return out
    }

    private fun detectGpu(): GpuPolicy? {
        val roots = listOf(
            "/sys/class/kgsl/kgsl-3d0",
            "/sys/devices/platform/soc@0/3d00000.gpu/kgsl/kgsl-3d0",
        )
        val root = roots.firstOrNull { readText("$it/max_pwrlevel") != null } ?: return null
        val raw = readText("$root/gpu_available_frequencies")
            ?: readText("$root/devfreq/available_frequencies")
            ?: return null
        val freqs = raw.split(Regex("\\s+"))
            .mapNotNull { it.toLongOrNull()?.let { hz -> (hz / 1000L).toInt() } }
            .filter { it > 0 }
            .distinct()
            .sorted()
        if (freqs.size < 2) return null
        val maxPwrPath = "$root/max_pwrlevel"
        val minPwrPath = "$root/min_pwrlevel"
        val maxPwr = readInt(maxPwrPath) ?: 0
        val current = freqs.getOrElse((freqs.size - 1 - maxPwr).coerceIn(0, freqs.lastIndex)) {
            freqs.first()
        }
        return GpuPolicy(
            freqs = freqs,
            minKhz = freqs.first(),
            maxKhz = freqs.last(),
            currentKhz = current,
            minPwrPath = minPwrPath,
            maxPwrPath = maxPwrPath,
            curPath = "$root/gpuclk",
        )
    }

    private fun findThermalZone(): String {
        val base = "/sys/class/thermal"
        var fallback = ""
        for (zone in listDir(base).filter { it.startsWith("thermal_zone") }.sorted()) {
            val path = "$base/$zone"
            val tempPath = "$path/temp"
            if (readText(tempPath) == null) continue
            if (fallback.isEmpty()) fallback = tempPath
            val type = readText("$path/type")?.lowercase().orEmpty()
            if (type.contains("soc") || type.contains("cpu") || type.contains("tsens")) {
                return tempPath
            }
        }
        return fallback
    }

    // ----------------------------------------------------------------- io

    private fun firstReadable(vararg paths: String): String =
        paths.firstOrNull { readText(it) != null } ?: ""

    private fun readText(path: String): String? {
        if (path.isBlank()) return null
        try {
            val file = File(path)
            if (file.canRead()) {
                val text = file.readText().trim()
                if (text.isNotEmpty()) return text
            }
        } catch (_: Throwable) {
        }
        // A few vendor builds restrict direct reads from an app UID; PServer can
        // cat the same node as root without us needing any other privilege.
        if (binder == null) return null
        return try {
            exec("cat $path").trim().takeIf { it.isNotEmpty() }
        } catch (_: Throwable) {
            null
        }
    }

    private fun readInt(path: String): Int? = readText(path)?.trim()?.toIntOrNull()

    private fun listDir(path: String): List<String> {
        try {
            val direct = File(path).list()
            if (!direct.isNullOrEmpty()) return direct.toList()
        } catch (_: Throwable) {
        }
        if (binder == null) return emptyList()
        return try {
            exec("ls $path")
                .lines()
                .map { it.trim() }
                .filter { it.isNotEmpty() }
        } catch (_: Throwable) {
            emptyList()
        }
    }

    private fun parseFreqs(raw: String?): List<Int> = raw.orEmpty()
        .split(Regex("\\s+"))
        .mapNotNull { it.toIntOrNull() }
        .filter { it > 0 }
        .distinct()
        .sorted()

    private fun parseIds(raw: String?): List<Int> = raw.orEmpty()
        .split(Regex("\\s+"))
        .mapNotNull { it.toIntOrNull() }
        .distinct()

    private fun exec(script: String): String {
        val service = binder ?: error("PServer unavailable")
        val data = Parcel.obtain()
        val reply = Parcel.obtain()
        return try {
            data.writeStringArray(arrayOf(script, "1"))
            service.transact(0, data, reply, 0)
            reply.createByteArray()?.toString(Charsets.UTF_8)?.trim().orEmpty()
        } finally {
            data.recycle()
            reply.recycle()
        }
    }

    private fun socModel(): String = try {
        if (Build.VERSION.SDK_INT >= 31) Build.SOC_MODEL ?: "" else ""
    } catch (_: Throwable) {
        ""
    }
}
