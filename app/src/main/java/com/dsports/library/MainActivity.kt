package com.dsports.library

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.DocumentsContract
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import java.util.Locale

class MainActivity : Activity() {
    private val pickFolder = 101
    private lateinit var list: LinearLayout
    private lateinit var status: TextView
    private var folderUri: Uri? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.decorView.systemUiVisibility = (View.SYSTEM_UI_FLAG_FULLSCREEN or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(20, 20, 20, 20)
            setBackgroundColor(0xFF10151D.toInt())
        }
        val left = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(12, 12, 12, 12)
            setBackgroundColor(0xFF192331.toInt())
        }
        val title = TextView(this).apply { text = "DSPORTS"; textSize = 26f; setTextColor(0xFFFFFFFF.toInt()) }
        status = TextView(this).apply { text = "Choose your DS folder. Files are read-only."; textSize = 14f; setTextColor(0xFFB8C7D9.toInt()); setPadding(0, 12, 0, 12) }
        val choose = Button(this).apply { text = "Select DS folder"; setOnClickListener { chooseFolder() } }
        left.addView(title)
        left.addView(status)
        left.addView(choose)
        val right = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(18, 12, 12, 12)
            setBackgroundColor(0xFF0D1117.toInt())
        }
        right.addView(TextView(this).apply {
            text = "GAME LIBRARY"; textSize = 20f; setTextColor(0xFFFFFFFF.toInt())
        })
        list = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        right.addView(ScrollView(this).apply { addView(list) }, LinearLayout.LayoutParams(-1, 0, 1f))
        root.addView(left, LinearLayout.LayoutParams(0, -1, 0.38f))
        root.addView(right, LinearLayout.LayoutParams(0, -1, 0.62f))
        setContentView(root)
        addMessage("No folder selected", "Select the folder containing your .nds files.")
    }

    private fun chooseFolder() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
        }
        startActivityForResult(intent, pickFolder)
    }

    @Deprecated("Legacy activity result API for broad Android compatibility")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != pickFolder || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        try {
            contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
            folderUri = uri
            scan(uri)
        } catch (e: Exception) {
            status.text = "Could not read folder: ${e.message ?: "permission denied"}"
        }
    }

    private fun scan(tree: Uri) {
        list.removeAllViews()
        val found = mutableListOf<Pair<String, Uri>>()
        try {
            val rootId = DocumentsContract.getTreeDocumentId(tree)
            scanDirectory(tree, rootId, found, 0)
            found.sortBy { it.first.lowercase(Locale.ROOT) }
            status.text = "Read-only scan complete · ${found.size} DS file(s)"
            if (found.isEmpty()) addMessage("No .nds files found", "Choose the folder that contains your Nintendo DS game files.")
            found.forEach { (name, uri) ->
                val soulSilver = name.lowercase(Locale.ROOT).let { it.contains("soulsilver") || it.contains("soul silver") || it.contains("pokemon_soul") || it.contains("pokémon_soul") }
                val card = LinearLayout(this).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(14, 12, 14, 12)
                    setBackgroundColor(0xFF192331.toInt())
                }
                card.addView(TextView(this).apply {
                    text = name; textSize = 17f; setTextColor(0xFFFFFFFF.toInt())
                })
                card.addView(TextView(this).apply {
                    text = if (soulSilver) "Pokémon SoulSilver · DS ROM detected" else "Nintendo DS game file"
                    textSize = 13f; setTextColor(0xFFB8C7D9.toInt()); setPadding(0, 5, 0, 6)
                })
                if (soulSilver) card.addView(Button(this).apply {
                    text = "Open game"
                    setOnClickListener {
                        Toast.makeText(this@MainActivity, "This prototype scans files only. A DS runtime is not bundled yet; no ROM has been modified.", Toast.LENGTH_LONG).show()
                    }
                })
                else card.addView(TextView(this).apply {
                    text = "Runtime not configured"; textSize = 12f; setTextColor(0xFF8292A6.toInt())
                })
                val params = LinearLayout.LayoutParams(-1, -2)
                params.bottomMargin = 12
                list.addView(card, params)
            }
        } catch (e: Exception) {
            status.text = "Scan failed: ${e.message ?: "unable to read folder"}"
            addMessage("Could not scan folder", "Check that Android granted read access to this folder.")
        }
    }

    private fun scanDirectory(tree: Uri, documentId: String, output: MutableList<Pair<String, Uri>>, depth: Int) {
        if (depth > 8 || output.size > 2000) return
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, documentId)
        val projection = arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_MIME_TYPE)
        contentResolver.query(childrenUri, projection, null, null, null)?.use { cursor ->
            val idCol = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DOCUMENT_ID)
            val nameCol = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
            val mimeCol = cursor.getColumnIndexOrThrow(DocumentsContract.Document.COLUMN_MIME_TYPE)
            while (cursor.moveToNext()) {
                val id = cursor.getString(idCol) ?: continue
                val name = cursor.getString(nameCol) ?: continue
                val mime = cursor.getString(mimeCol)
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                    scanDirectory(tree, id, output, depth + 1)
                } else if (name.lowercase(Locale.ROOT).endsWith(".nds")) {
                    output.add(name to DocumentsContract.buildDocumentUriUsingTree(tree, id))
                }
            }
        }
    }

    private fun addMessage(heading: String, detail: String) {
        list.addView(TextView(this).apply {
            text = "$heading\n$detail"; textSize = 15f; setTextColor(0xFFB8C7D9.toInt()); setPadding(0, 20, 0, 20)
        })
    }
}
