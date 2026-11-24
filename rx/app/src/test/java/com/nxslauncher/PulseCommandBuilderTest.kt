package com.nxslauncher

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The PServer script is the only part of PULSE that can brick a session's
 * clocks, so its rules are pinned here: widen a pinned floor before capping,
 * snap to real OPPs, and restore the values found at probe time.
 */
class PulseCommandBuilderTest {

    private val freqs = listOf(300_000, 600_000, 900_000)

    private fun target(
        id: Int = 0,
        min: Int = 300_000,
        max: Int = 900_000,
        minNow: Int = 300_000,
    ) = PulseCommandBuilder.Target(
        id = id,
        freqs = freqs,
        minKhz = min,
        maxKhz = max,
        currentMinKhz = minNow,
        minPath = "/sys/policy$id/scaling_min_freq",
        maxPath = "/sys/policy$id/scaling_max_freq",
    )

    private val gpu = PulseCommandBuilder.Gpu(
        freqs = listOf(160_000, 400_000, 800_000),
        minPwrPath = "/gpu/min_pwrlevel",
        maxPwrPath = "/gpu/max_pwrlevel",
    )

    @Test
    fun capWritesFrequencyAndLocksTheNode() {
        val script = PulseCommandBuilder.build(
            restore = false,
            caps = mapOf(0 to 600_000),
            targets = listOf(target()),
            gpu = null,
            gpuKhz = 0,
        )
        assertTrue(script.startsWith("#!/system/bin/sh"))
        assertTrue(script.contains("chmod 666 /sys/policy0/scaling_max_freq"))
        assertTrue(script.contains("echo 600000 > /sys/policy0/scaling_max_freq"))
        assertTrue(script.contains("chmod 444 /sys/policy0/scaling_max_freq"))
        // The floor is already low enough, so it must be left alone.
        assertFalse(script.contains("scaling_min_freq"))
    }

    @Test
    fun pinnedPrimeFloorIsLoweredBeforeTheCap() {
        val script = PulseCommandBuilder.build(
            restore = false,
            caps = mapOf(7 to 600_000),
            targets = listOf(target(id = 7, minNow = 900_000)),
            gpu = null,
            gpuKhz = 0,
        )
        val minAt = script.indexOf("echo 300000 > /sys/policy7/scaling_min_freq")
        val maxAt = script.indexOf("echo 600000 > /sys/policy7/scaling_max_freq")
        assertTrue("floor must be widened first", minAt in 0 until maxAt)
    }

    @Test
    fun unsupportedValueSnapsToTheNearestOpp() {
        val script = PulseCommandBuilder.build(
            restore = false,
            caps = mapOf(0 to 490_000),
            targets = listOf(target()),
            gpu = null,
            gpuKhz = 0,
        )
        assertTrue(script.contains("echo 600000 > /sys/policy0/scaling_max_freq"))
        assertEquals(600_000, PulseCommandBuilder.nearest(freqs, 490_000))
        assertEquals(300_000, PulseCommandBuilder.nearest(freqs, 450_000))
    }

    @Test
    fun gpuWidensMinPowerLevelBeforeRaisingTheCap() {
        val script = PulseCommandBuilder.build(
            restore = false,
            caps = emptyMap(),
            targets = emptyList(),
            gpu = gpu,
            gpuKhz = 400_000,
        )
        val widen = script.indexOf("echo 2 > /gpu/min_pwrlevel")
        val cap = script.indexOf("echo 1 > /gpu/max_pwrlevel")
        assertTrue("min power level must be widened first", widen in 0 until cap)
        assertTrue(script.contains("chmod 444 /gpu/max_pwrlevel"))
    }

    @Test
    fun powerLevelMappingIsFastestFirst() {
        assertEquals(0, PulseCommandBuilder.levelFor(listOf(160_000, 400_000, 800_000), 800_000))
        assertEquals(1, PulseCommandBuilder.levelFor(listOf(160_000, 400_000, 800_000), 400_000))
        assertEquals(2, PulseCommandBuilder.levelFor(listOf(160_000, 400_000, 800_000), 160_000))
    }

    @Test
    fun restoreReturnsTheOriginalValuesAndWritability() {
        val script = PulseCommandBuilder.build(
            restore = true,
            caps = mapOf(0 to 600_000),
            targets = listOf(target()),
            gpu = gpu,
            gpuKhz = 400_000,
        )
        assertTrue(script.contains("echo 900000 > /sys/policy0/scaling_max_freq"))
        assertTrue(script.contains("chmod 644 /sys/policy0/scaling_max_freq"))
        assertTrue(script.contains("echo 0 > /gpu/max_pwrlevel"))
        assertTrue(script.contains("echo 2 > /gpu/min_pwrlevel"))
        assertFalse(script.contains("chmod 444"))
    }

    @Test
    fun missingCapLeavesItsClusterUntouched() {
        val script = PulseCommandBuilder.build(
            restore = false,
            caps = mapOf(4 to 600_000),
            targets = listOf(target(id = 0), target(id = 4)),
            gpu = null,
            gpuKhz = 0,
        )
        assertFalse(script.contains("/sys/policy0/"))
        assertTrue(script.contains("/sys/policy4/"))
    }

    @Test
    fun zeroOrNegativeCapsAreIgnored() {
        val script = PulseCommandBuilder.build(
            restore = false,
            caps = mapOf(0 to 0),
            targets = listOf(target()),
            gpu = null,
            gpuKhz = 0,
        )
        assertEquals("#!/system/bin/sh", script)
    }
}
