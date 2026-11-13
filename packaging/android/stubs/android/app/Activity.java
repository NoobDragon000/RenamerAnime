package android.app;

import android.content.ContentResolver;
import android.content.Intent;
import android.os.Bundle;
import android.view.View;
import java.io.File;

public class Activity extends android.content.Context {
    public static final int RESULT_OK = -1;
    protected void onCreate(Bundle savedInstanceState) {}
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {}
    public void startActivityForResult(Intent intent, int requestCode) {}
    public void setContentView(View view) {}
    public void runOnUiThread(Runnable action) {}
    public ContentResolver getContentResolver() { return null; }
    public android.content.res.AssetManager getAssets() { return null; }
    public File getFilesDir() { return null; }
    public void finish() {}
}
