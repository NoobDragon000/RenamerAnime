package io.github.noobdragon000.renameranime

import android.content.Intent
import android.database.Cursor
import android.net.Uri
import android.provider.DocumentsContract
import android.webkit.JavascriptInterface
import android.webkit.WebView
import org.json.JSONArray
import org.json.JSONObject

/**
 * Storage Access Framework (SAF) bridge exposed to the Tauri WebView as `window.AndroidSafBridge`.
 * All Android directory traversal and file/folder renames operate exclusively through
 * `content://` URIs and `DocumentsContract` rather than raw filesystem paths.
 */
class SafBridge(
    private val activity: MainActivity,
    private val webView: WebView,
) {
    @JavascriptInterface
    fun pickDocumentTree() {
        activity.runOnUiThread {
            activity.launchDocumentTreePicker()
        }
    }

    fun notifyTreePicked(uri: Uri?) {
        val jsArg = if (uri == null) "null" else JSONObject.quote(uri.toString())
        webView.post {
            webView.evaluateJavascript("window.__onSafTreePicked && window.__onSafTreePicked($jsArg);", null)
        }
    }

    @JavascriptInterface
    fun listTreeEntries(treeUriString: String, recursive: Boolean): String {
        val results = JSONArray()
        return try {
            val treeUri = Uri.parse(treeUriString)
            val rootDocId = DocumentsContract.getTreeDocumentId(treeUri)
            val rootDocUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, rootDocId)
            traverseChildren(
                treeUri = treeUri,
                parentDocId = rootDocId,
                parentUri = rootDocUri.toString(),
                depth = 0,
                recursive = recursive,
                out = results,
            )
            results.toString()
        } catch (e: Exception) {
            JSONObject()
                .put("error", e.message ?: "Failed to query SAF document tree")
                .toString()
        }
    }

    private fun traverseChildren(
        treeUri: Uri,
        parentDocId: String,
        parentUri: String,
        depth: Int,
        recursive: Boolean,
        out: JSONArray,
    ) {
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocId)
        val projection = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
        )

        val resolver = activity.contentResolver
        val cursor: Cursor? = resolver.query(childrenUri, projection, null, null, null)
        cursor?.use { c ->
            val idIdx = c.getColumnIndex(DocumentsContract.Document.COLUMN_DOCUMENT_ID)
            val nameIdx = c.getColumnIndex(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
            val mimeIdx = c.getColumnIndex(DocumentsContract.Document.COLUMN_MIME_TYPE)

            val subdirs = mutableListOf<Pair<String, String>>()
            while (c.moveToNext()) {
                val docId = if (idIdx >= 0) c.getString(idIdx) else continue
                val name = if (nameIdx >= 0) c.getString(nameIdx) else continue
                val mime = if (mimeIdx >= 0) c.getString(mimeIdx) else ""
                val isDir = mime == DocumentsContract.Document.MIME_TYPE_DIR
                val docUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId).toString()

                val obj = JSONObject()
                    .put("uri", docUri)
                    .put("parent_uri", parentUri)
                    .put("name", name)
                    .put("is_dir", isDir)
                    .put("depth", depth)
                out.put(obj)

                if (isDir && recursive) {
                    subdirs.add(docId to docUri)
                }
            }

            for ((subDocId, subDocUri) in subdirs) {
                traverseChildren(
                    treeUri = treeUri,
                    parentDocId = subDocId,
                    parentUri = subDocUri,
                    depth = depth + 1,
                    recursive = true,
                    out = out,
                )
            }
        }
    }

    @JavascriptInterface
    fun renameDocument(documentUriString: String, newDisplayName: String): String {
        return try {
            val docUri = Uri.parse(documentUriString)
            val renamedUri = DocumentsContract.renameDocument(activity.contentResolver, docUri, newDisplayName)
            if (renamedUri != null) {
                JSONObject()
                    .put("ok", true)
                    .put("new_uri", renamedUri.toString())
                    .toString()
            } else {
                JSONObject()
                    .put("ok", false)
                    .put("error", "DocumentsContract.renameDocument returned null")
                    .toString()
            }
        } catch (e: Exception) {
            JSONObject()
                .put("ok", false)
                .put("error", e.message ?: "SAF rename failed")
                .toString()
        }
    }
}
