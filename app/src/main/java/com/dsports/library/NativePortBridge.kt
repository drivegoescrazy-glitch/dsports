package com.dsports.library

/**
 * JNI entry point for the native runtime bootstrap.
 *
 * This bridge confirms that the Android NDK library loads. It does not yet
 * execute SoulSilver code or emulate Nintendo DS hardware.
 */
object NativePortBridge {
    init {
        System.loadLibrary("dsports_native")
    }

    external fun nativeStatus(): String
}
