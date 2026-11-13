package android.content;

import android.net.Uri;

public class Intent {
    public static final String ACTION_OPEN_DOCUMENT_TREE = "android.intent.action.OPEN_DOCUMENT_TREE";
    public static final int FLAG_GRANT_READ_URI_PERMISSION = 1;
    public static final int FLAG_GRANT_WRITE_URI_PERMISSION = 2;
    public static final int FLAG_GRANT_PERSISTABLE_URI_PERMISSION = 64;
    public Intent(String action) {}
    public Intent addFlags(int flags) { return this; }
    public Uri getData() { return null; }
    public int getFlags() { return 0; }
}
