package com.infrawrench.schist;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.content.SharedPreferences;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import android.webkit.MimeTypeMap;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.util.Locale;
import java.util.UUID;

/** Read-only, per-file URI grants. No directory or arbitrary-path access. */
public final class VideoProvider extends ContentProvider {
    private static final String PREFS = "video-shares";
    private static final long LIFETIME = 7L * 24 * 60 * 60 * 1000;
    static Uri share(Context context, File source) throws IOException {
        File file = source.getCanonicalFile();
        if (!file.isFile()) throw new FileNotFoundException();
        String token = UUID.randomUUID().toString();
        SharedPreferences prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        SharedPreferences.Editor edit = prefs.edit();
        long now = System.currentTimeMillis();
        for (String key : prefs.getAll().keySet()) {
            if (key.endsWith(".time") && now - prefs.getLong(key, 0) > LIFETIME) {
                edit.remove(key).remove(key.substring(0, key.length() - 5));
            }
        }
        if (!edit.putString(token, file.getPath()).putLong(token + ".time", now).commit())
            throw new IOException("video.editor_unavailable");
        return new Uri.Builder().scheme("content").authority(context.getPackageName() + ".video")
            .appendPath(token).appendPath(file.getName()).build();
    }
    private File file(Uri uri) throws FileNotFoundException {
        if (uri.getPathSegments().size() != 2) throw new FileNotFoundException();
        String token = uri.getPathSegments().get(0);
        SharedPreferences prefs = getContext().getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        String path = prefs.getString(token, null);
        if (path == null || System.currentTimeMillis() - prefs.getLong(token + ".time", 0) > LIFETIME)
            throw new FileNotFoundException();
        File file = new File(path);
        if (!file.isFile() || !file.getName().equals(uri.getLastPathSegment())) throw new FileNotFoundException();
        return file;
    }
    static String mime(File file) {
        String name = file.getName();
        int dot = name.lastIndexOf('.');
        String mime = dot < 0 ? null : MimeTypeMap.getSingleton().getMimeTypeFromExtension(name.substring(dot + 1).toLowerCase(Locale.ROOT));
        return mime != null && mime.startsWith("video/") ? mime : "video/mp4";
    }
    @Override public boolean onCreate() { return true; }
    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!"r".equals(mode)) throw new FileNotFoundException();
        return ParcelFileDescriptor.open(file(uri), ParcelFileDescriptor.MODE_READ_ONLY);
    }
    @Override public String getType(Uri uri) {
        try { return mime(file(uri)); } catch (FileNotFoundException error) { return null; }
    }
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] args, String order) {
        try {
            File file = file(uri);
            String[] columns = projection == null ? new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE} : projection;
            MatrixCursor cursor = new MatrixCursor(columns);
            MatrixCursor.RowBuilder row = cursor.newRow();
            for (String column : columns) row.add(OpenableColumns.DISPLAY_NAME.equals(column) ? file.getName()
                : OpenableColumns.SIZE.equals(column) ? file.length() : null);
            return cursor;
        } catch (FileNotFoundException error) { return null; }
    }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] args) { throw new UnsupportedOperationException(); }
}
