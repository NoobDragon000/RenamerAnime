package io.github.noobdragon000.renameranime;

import android.content.ContentResolver;
import android.database.Cursor;
import android.net.Uri;
import android.provider.DocumentsContract;
import android.webkit.JavascriptInterface;
import android.webkit.WebView;
import java.util.ArrayList;
import java.util.List;
import org.json.JSONArray;
import org.json.JSONObject;

public class SafBridge {
    private final MainActivity activity;
    private final WebView webView;

    public SafBridge(MainActivity activity, WebView webView) {
        this.activity = activity;
        this.webView = webView;
    }

    @JavascriptInterface
    public void pickDocumentTree() {
        activity.runOnUiThread(new Runnable() {
            @Override
            public void run() {
                activity.launchDocumentTreePicker();
            }
        });
    }

    public void notifyTreePicked(Uri uri) {
        final String jsArg = uri == null ? "null" : JSONObject.quote(uri.toString());
        webView.post(new Runnable() {
            @Override
            public void run() {
                webView.evaluateJavascript(
                    "window.__onSafTreePicked && window.__onSafTreePicked(" + jsArg + ");",
                    null
                );
            }
        });
    }

    @JavascriptInterface
    public String listTreeEntries(String treeUriString, boolean recursive) {
        JSONArray results = new JSONArray();
        try {
            Uri treeUri = Uri.parse(treeUriString);
            String rootDocId = DocumentsContract.getTreeDocumentId(treeUri);
            String rootDocUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, rootDocId).toString();
            traverseChildren(treeUri, rootDocId, rootDocUri, 0, recursive, results);
            return results.toString();
        } catch (Exception e) {
            try {
                return new JSONObject()
                    .put("error", e.getMessage() != null ? e.getMessage() : "SAF tree read failed")
                    .toString();
            } catch (Exception ignored) {
                return "{\"error\":\"SAF tree read failed\"}";
            }
        }
    }

    private void traverseChildren(
        Uri treeUri,
        String parentDocId,
        String parentUri,
        int depth,
        boolean recursive,
        JSONArray out
    ) throws Exception {
        Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentDocId);
        String[] projection = new String[] {
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE
        };

        ContentResolver resolver = activity.getContentResolver();
        Cursor cursor = resolver.query(childrenUri, projection, null, null, null);
        if (cursor == null) {
            return;
        }
        List<String[]> subdirs = new ArrayList<String[]>();
        try {
            int idIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_DOCUMENT_ID);
            int nameIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_DISPLAY_NAME);
            int mimeIdx = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_MIME_TYPE);

            while (cursor.moveToNext()) {
                if (idIdx < 0 || nameIdx < 0) continue;
                String docId = cursor.getString(idIdx);
                String name = cursor.getString(nameIdx);
                String mime = mimeIdx >= 0 ? cursor.getString(mimeIdx) : "";
                boolean isDir = DocumentsContract.Document.MIME_TYPE_DIR.equals(mime);
                String docUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, docId).toString();

                JSONObject obj = new JSONObject();
                obj.put("uri", docUri);
                obj.put("parent_uri", parentUri);
                obj.put("name", name);
                obj.put("is_dir", isDir);
                obj.put("depth", depth);
                out.put(obj);

                if (isDir && recursive) {
                    subdirs.add(new String[] { docId, docUri });
                }
            }
        } finally {
            cursor.close();
        }

        for (String[] sub : subdirs) {
            traverseChildren(treeUri, sub[0], sub[1], depth + 1, true, out);
        }
    }

    @JavascriptInterface
    public String renameDocument(String documentUriString, String newDisplayName) {
        try {
            Uri docUri = Uri.parse(documentUriString);
            Uri renamed = DocumentsContract.renameDocument(
                activity.getContentResolver(),
                docUri,
                newDisplayName
            );
            if (renamed != null) {
                return new JSONObject()
                    .put("ok", true)
                    .put("new_uri", renamed.toString())
                    .toString();
            }
            return new JSONObject()
                .put("ok", false)
                .put("error", "DocumentsContract.renameDocument returned null")
                .toString();
        } catch (Exception e) {
            try {
                return new JSONObject()
                    .put("ok", false)
                    .put("error", e.getMessage() != null ? e.getMessage() : "SAF rename failed")
                    .toString();
            } catch (Exception ignored) {
                return "{\"ok\":false,\"error\":\"SAF rename failed\"}";
            }
        }
    }
}
