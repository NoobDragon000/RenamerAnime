package io.github.noobdragon000.renameranime;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import java.io.InputStream;

public class MainActivity extends Activity {
    private static final int REQUEST_CODE_OPEN_TREE = 4091;
    private static final String ASSET_HOST = "appassets.renamer.local";

    private WebView webView;
    private SafBridge safBridge;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        webView = new WebView(this);
        webView.setBackgroundColor(0xFF0F1117);

        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(true);

        safBridge = new SafBridge(this, webView);
        AndroidBackend backend = new AndroidBackend(this, webView);

        webView.addJavascriptInterface(safBridge, "AndroidSafBridge");
        webView.addJavascriptInterface(backend, "AndroidNativeBackend");

        webView.setWebViewClient(new WebViewClient() {
            @Override
            public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
                Uri url = request.getUrl();
                if (url != null && ASSET_HOST.equals(url.getPath() == null ? null : getHost(url))) {
                    String path = url.getPath();
                    if (path == null || path.equals("/") || path.isEmpty()) {
                        path = "/index.html";
                    }
                    String assetPath = "www" + path;
                    try {
                        InputStream stream = getAssets().open(assetPath);
                        return new WebResourceResponse(guessMime(assetPath), "UTF-8", stream);
                    } catch (Exception ignored) {
                        try {
                            InputStream fallback = getAssets().open("www/index.html");
                            return new WebResourceResponse("text/html", "UTF-8", fallback);
                        } catch (Exception ignored2) {
                        }
                    }
                }
                return super.shouldInterceptRequest(view, request);
            }

            private String getHost(Uri url) {
                String raw = url.toString();
                int schemeEnd = raw.indexOf("://");
                if (schemeEnd < 0) return "";
                int start = schemeEnd + 3;
                int slash = raw.indexOf('/', start);
                return slash > start ? raw.substring(start, slash) : raw.substring(start);
            }

            private String guessMime(String path) {
                if (path.endsWith(".html")) return "text/html";
                if (path.endsWith(".js") || path.endsWith(".mjs")) return "application/javascript";
                if (path.endsWith(".css")) return "text/css";
                if (path.endsWith(".svg")) return "image/svg+xml";
                if (path.endsWith(".png")) return "image/png";
                if (path.endsWith(".json")) return "application/json";
                return "application/octet-stream";
            }
        });

        setContentView(webView);
        webView.loadUrl("https://" + ASSET_HOST + "/index.html");
    }

    public void launchDocumentTreePicker() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        intent.addFlags(
            Intent.FLAG_GRANT_READ_URI_PERMISSION
                | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
        );
        startActivityForResult(intent, REQUEST_CODE_OPEN_TREE);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQUEST_CODE_OPEN_TREE) {
            Uri uri = (resultCode == RESULT_OK && data != null) ? data.getData() : null;
            if (uri != null) {
                try {
                    int flags = Intent.FLAG_GRANT_READ_URI_PERMISSION
                        | Intent.FLAG_GRANT_WRITE_URI_PERMISSION;
                    getContentResolver().takePersistableUriPermission(uri, flags);
                } catch (SecurityException ignored) {
                }
            }
            if (safBridge != null) {
                safBridge.notifyTreePicked(uri);
            }
        }
    }
}
