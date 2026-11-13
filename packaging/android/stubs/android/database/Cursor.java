package android.database;

import java.io.Closeable;

public interface Cursor extends Closeable {
    boolean moveToNext();
    int getColumnIndex(String columnName);
    String getString(int columnIndex);
    @Override
    void close();
}
