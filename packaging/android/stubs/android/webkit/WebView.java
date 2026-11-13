package android.webkit;

import android.content.Context;
import android.view.View;

public class WebView extends View {
    public WebView(Context context) {}
    public WebSettings getSettings() { return null; }
    public void setBackgroundColor(int color) {}
    public void setWebViewClient(WebViewClient client) {}
    public void addJavascriptInterface(Object object, String name) {}
    public void loadUrl(String url) {}
    public void evaluateJavascript(String script, ValueCallback<String> resultCallback) {}
    public boolean post(Runnable action) { return true; }
    public boolean canGoBack() { return false; }
    public void goBack() {}
}
