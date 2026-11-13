package io.github.noobdragon000.renameranime

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.webkit.WebView
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts

/**
 * Tauri Android Activity hosting the WebView and registering `window.AndroidSafBridge`
 * plus the `ACTION_OPEN_DOCUMENT_TREE` launcher for persistent `content://` folder grants.
 */
class MainActivity : TauriActivity() {
    private var safBridge: SafBridge? = null
    private lateinit var treePickerLauncher: ActivityResultLauncher<Uri?>

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        treePickerLauncher = registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
            if (uri != null) {
                val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                try {
                    contentResolver.takePersistableUriPermission(uri, flags)
                } catch (_: SecurityException) {
                    // Some document providers issue temporary grants without persistable support.
                }
            }
            safBridge?.notifyTreePicked(uri)
        }
    }

    override fun onWebViewCreate(webView: WebView) {
        super.onWebViewCreate(webView)
        val bridge = SafBridge(this, webView)
        safBridge = bridge
        webView.addJavascriptInterface(bridge, "AndroidSafBridge")
    }

    fun launchDocumentTreePicker() {
        treePickerLauncher.launch(null)
    }
}
