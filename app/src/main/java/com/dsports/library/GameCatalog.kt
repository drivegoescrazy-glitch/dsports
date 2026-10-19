package com.dsports.library

import android.content.Context

/**
 * One game DSports can build a native port for.
 *
 * A game is only listed here once DSports has a working, validated pipeline for
 * it. The catalogue is intentionally small: SoulSilver is the only supported
 * title today, and listing more games before their pipeline works would only
 * produce failed builds.
 */
data class SupportedGame(
    val id: String,
    val title: String,
    val subtitle: String,
    val gameCode: String,
    val region: String,
    val requiredFileName: String,
    val available: Boolean,
    val statusNote: String
)

object GameCatalog {
    const val EXTRA_GAME_ID = "com.dsports.library.GAME_ID"
    const val DEFAULT_GAME_ID = "soulsilver-us"

    val games: List<SupportedGame> = listOf(
        SupportedGame(
            id = DEFAULT_GAME_ID,
            title = "Pokémon SoulSilver",
            subtitle = "Nintendo DS · native recompilation",
            gameCode = "IPGE",
            region = "USA",
            requiredFileName = "SoulSilver.nds",
            available = true,
            statusNote = "Supported. Choose your own US .nds dump to build the port locally."
        )
    )

    fun byId(id: String?): SupportedGame =
        games.firstOrNull { it.id == id } ?: games.first { it.available }
}

/** Screen layouts the built port can start with. */
enum class PortLayout(val key: String, val label: String, val description: String) {
    MERGED("merged", "Merged", "One view that combines the useful parts of both screens."),
    SIDE("side", "Dual · side by side", "Both DS screens next to each other. Best on wide displays."),
    VERTICAL("vertical", "Dual · stacked", "The classic stacked DS arrangement."),
    HYBRID("hybrid", "Hybrid", "One large screen with the other as a small inset.")
}

/** World render detail. The DS itself draws at 192 lines. */
enum class PortResolution(val key: String, val lines: Int, val label: String) {
    NATIVE("native", 192, "Native (192)"),
    R540("540", 540, "540 lines"),
    R720("720", 720, "720 lines"),
    R1080("1080", 1080, "1080 lines"),
    R1440("1440", 1440, "1440 lines")
}

/**
 * Display settings chosen in DSports and baked into the port as its defaults.
 * The port's own pause menu can still change them at any time.
 */
data class PortDisplay(
    val layout: PortLayout = PortLayout.MERGED,
    val resolution: PortResolution = PortResolution.R720,
    val smoothScaling: Boolean = false,
    val widescreenWorld: Boolean = true
) {
    fun toBuilderArguments(): List<String> = buildList {
        add("--layout"); add(layout.key)
        add("--resolution"); add(resolution.key)
        if (smoothScaling) add("--smooth")
        add(if (widescreenWorld) "--widescreen" else "--no-widescreen")
    }

    companion object {
        private const val PREFS = "dsports-port-display"
        private const val KEY_LAYOUT = "layout"
        private const val KEY_RESOLUTION = "resolution"
        private const val KEY_SMOOTH = "smooth"
        private const val KEY_WIDE = "widescreen"

        fun load(context: Context): PortDisplay {
            val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val layout = PortLayout.entries.firstOrNull { it.key == prefs.getString(KEY_LAYOUT, null) }
                ?: PortLayout.MERGED
            val resolution = PortResolution.entries.firstOrNull { it.key == prefs.getString(KEY_RESOLUTION, null) }
                ?: PortResolution.R720
            return PortDisplay(
                layout = layout,
                resolution = resolution,
                smoothScaling = prefs.getBoolean(KEY_SMOOTH, false),
                widescreenWorld = prefs.getBoolean(KEY_WIDE, true)
            )
        }

        fun save(context: Context, display: PortDisplay) {
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString(KEY_LAYOUT, display.layout.key)
                .putString(KEY_RESOLUTION, display.resolution.key)
                .putBoolean(KEY_SMOOTH, display.smoothScaling)
                .putBoolean(KEY_WIDE, display.widescreenWorld)
                .apply()
        }
    }
}
