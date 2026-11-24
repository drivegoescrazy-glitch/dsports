package com.nxslauncher

import kotlin.math.abs

/**
 * Builds the shell script PULSE hands to PServer.
 *
 * The no-root technique (write `scaling_max_freq` for each CPU policy, choose an
 * Adreno power level in `max_pwrlevel`) comes from ClusterTune and is documented
 * by keiretrogaming/pulse. Keeping the script construction pure makes it
 * unit-testable without a device or the privileged service.
 */
object PulseCommandBuilder {

    data class Target(
        val id: Int,
        val freqs: List<Int>,
        val minKhz: Int,
        val maxKhz: Int,
        val currentMinKhz: Int,
        val minPath: String,
        val maxPath: String,
    )

    data class Gpu(
        val freqs: List<Int>,
        val minPwrPath: String,
        val maxPwrPath: String,
    )

    fun build(
        restore: Boolean,
        caps: Map<Int, Int>,
        targets: List<Target>,
        gpu: Gpu?,
        gpuKhz: Int,
    ): String {
        val lines = mutableListOf("#!/system/bin/sh")
        if (restore) {
            // Hand every ceiling back at full range and leave the nodes
            // writable, so the vendor's own performance service takes over.
            targets.forEach { target ->
                val top = target.freqs.lastOrNull()?.takeIf { it > 0 } ?: target.maxKhz
                if (top > 0) write(lines, target.maxPath, top, "644")
            }
            if (gpu != null && gpu.freqs.size > 1) {
                write(lines, gpu.minPwrPath, gpu.freqs.size - 1, "644")
                write(lines, gpu.maxPwrPath, 0, "644")
            }
            return lines.joinToString("\n")
        }

        targets.forEach { target ->
            val wanted = caps[target.id] ?: return@forEach
            if (wanted <= 0) return@forEach
            val low = target.minKhz.coerceAtLeast(0)
            val high = target.maxKhz.coerceAtLeast(low)
            val cap = nearest(target.freqs, wanted).coerceIn(low, high)
            // A vendor HAL can pin the prime cluster's minimum near its top OPP;
            // the kernel rejects max < min, so widen the floor first in that case.
            if (target.currentMinKhz > cap) {
                write(lines, target.minPath, target.minKhz, "444")
            }
            write(lines, target.maxPath, cap, "444")
        }

        gpu?.let { device ->
            if (gpuKhz > 0 && device.freqs.size > 1) {
                val level = levelFor(device.freqs, gpuKhz)
                // max_pwrlevel can never be a slower index than min_pwrlevel.
                // Widen min to the slowest level so the ceiling always sticks.
                write(lines, device.minPwrPath, device.freqs.size - 1, "444")
                write(lines, device.maxPwrPath, level, "444")
            }
        }
        return lines.joinToString("\n")
    }

    /** Nearest supported step; ties go to the lower frequency. */
    fun nearest(freqs: List<Int>, wanted: Int): Int {
        if (freqs.isEmpty()) return wanted
        return freqs.minByOrNull { abs(it - wanted) } ?: wanted
    }

    /** Adreno power levels run fastest-first while the table is ascending. */
    fun levelFor(freqs: List<Int>, khz: Int): Int {
        if (freqs.isEmpty()) return 0
        val value = nearest(freqs, khz)
        val index = freqs.indexOf(value).coerceAtLeast(0)
        return (freqs.size - 1 - index).coerceIn(0, freqs.lastIndex)
    }

    private fun write(lines: MutableList<String>, path: String, value: Int, mode: String) {
        if (path.isBlank()) return
        lines += "chmod 666 $path"
        lines += "echo $value > $path"
        lines += "chmod $mode $path"
    }
}
