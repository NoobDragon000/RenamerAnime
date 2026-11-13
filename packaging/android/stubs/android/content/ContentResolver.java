package android.content;

import android.database.Cursor;
import android.net.Uri;

public class ContentResolver {
    public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs, String sortOrder) {
        return null;
    }
    public void takePersistableUriPermission(Uri uri, int modeFlags) {}
}
