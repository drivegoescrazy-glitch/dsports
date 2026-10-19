package com.dsports.library

import android.app.Activity
import android.content.Intent
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * The launcher: everything DSports can build a port for.
 *
 * It shows the whole supported-game catalogue, not just the game that was
 * played last, and marks which entries are actually supported today. Selecting
 * a game opens the builder for it.
 */
class LauncherActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.decorView.systemUiVisibility = (
            View.SYSTEM_UI_FLAG_FULLSCREEN or
                View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
            )

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(0xFF0D1117.toInt())
            setPadding(28, 22, 28, 18)
        }

        root.addView(TextView(this).apply {
            text = "DSPORTS"
            textSize = 30f
            setTextColor(0xFFFFFFFF.toInt())
        })
        root.addView(TextView(this).apply {
            text = "NATIVE PORT LAUNCHER"
            textSize = 12f
            letterSpacing = 0.18f
            setTextColor(0xFF76D5C7.toInt())
            setPadding(0, 2, 0, 14)
        })
        root.addView(TextView(this).apply {
            text = "Supported games"
            textSize = 19f
            setTextColor(0xFFE6EDF5.toInt())
            setPadding(0, 0, 0, 8)
        })

        val list = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        for (game in GameCatalog.games) list.addView(gameCard(game))

        list.addView(TextView(this).apply {
            text = "More titles will appear here as their native pipelines are verified. " +
                "A game is only listed as supported once DSports can build it end to end."
            textSize = 12f
            setTextColor(0xFF8B9BB0.toInt())
            setPadding(4, 14, 4, 0)
        })

        root.addView(ScrollView(this).apply {
            isFillViewport = true
            addView(list)
        }, LinearLayout.LayoutParams(-1, 0, 1f))

        root.addView(TextView(this).apply {
            text = "Each port is built from your own game dump on this device. Nothing is uploaded."
            textSize = 12f
            setTextColor(0xFF93A4B8.toInt())
            setPadding(4, 12, 4, 0)
        })

        setContentView(root)
    }

    private fun gameCard(game: SupportedGame): View {
        val card = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(20, 16, 20, 16)
            background = GradientDrawable().apply {
                cornerRadius = 14f
                setColor(if (game.available) 0xFF192331.toInt() else 0xFF141A22.toInt())
                setStroke(1, if (game.available) 0xFF2F4257.toInt() else 0xFF232B36.toInt())
            }
        }
        card.addView(TextView(this).apply {
            text = game.title
            textSize = 20f
            setTextColor(if (game.available) 0xFFFFFFFF.toInt() else 0xFF7C8794.toInt())
        })
        card.addView(TextView(this).apply {
            text = game.subtitle
            textSize = 13f
            setTextColor(0xFF9FB0C4.toInt())
            setPadding(0, 2, 0, 6)
        })
        card.addView(TextView(this).apply {
            text = game.region + " · " + game.gameCode
            textSize = 12f
            setTextColor(0xFF6F7F92.toInt())
        })
        card.addView(TextView(this).apply {
            text = game.statusNote
            textSize = 12f
            setTextColor(if (game.available) 0xFF76D5C7.toInt() else 0xFF7C8794.toInt())
            setPadding(0, 8, 0, 10)
        })

        if (game.available) {
            card.addView(Button(this).apply {
                text = "Open " + game.title
                setOnClickListener { openBuilder(game) }
            })
        } else {
            card.addView(TextView(this).apply {
                text = "COMING SOON"
                textSize = 12f
                letterSpacing = 0.12f
                gravity = Gravity.START
                setTextColor(0xFF6F7F92.toInt())
            })
        }
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(0, 0, 0, 12)
            addView(card, LinearLayout.LayoutParams(-1, -2))
        }
    }

    private fun openBuilder(game: SupportedGame) {
        startActivity(Intent(this, MainActivity::class.java).apply {
            putExtra(GameCatalog.EXTRA_GAME_ID, game.id)
        })
    }
}
