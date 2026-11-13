package android.provider;

import android.content.ContentResolver;
import android.net.Uri;
import java.io.FileNotFoundException;

public final class DocumentsContract {
    public static final class Document {
        public static final String COLUMN_DOCUMENT_ID = "document_id";
        public static final String COLUMN_DISPLAY_NAME = "_display_name";
        public static final String COLUMN_MIME_TYPE = "mime_type";
        public static final String MIME_TYPE_DIR = "vnd.android.document/directory";
    }
    public static String getTreeDocumentId(Uri documentUri) { return null; }
    public static Uri buildDocumentUriUsingTree(Uri treeUri, String documentId) { return null; }
    public static Uri buildChildDocumentsUriUsingTree(Uri treeUri, String parentDocumentId) { return null; }
    public static Uri renameDocument(ContentResolver content, Uri documentUri, String displayName) throws FileNotFoundException { return null; }
}
