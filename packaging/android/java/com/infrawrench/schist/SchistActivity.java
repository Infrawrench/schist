package com.infrawrench.schist;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.webkit.MimeTypeMap;
import java.io.*;
import java.util.Locale;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.Executors;
import java.util.concurrent.ExecutorService;
import org.json.JSONArray;
import org.json.JSONObject;

/** NativeActivity with the activity-result bridge needed by the media picker. */
@SuppressWarnings("deprecation") // NativeActivity uses the platform activity-result API.
public final class SchistActivity extends NativeActivity {
    private static final int PICK_MEDIA = 41;
    private static final ConcurrentLinkedQueue<JSONObject> imports = new ConcurrentLinkedQueue<>();
    private static final ExecutorService copies = Executors.newSingleThreadExecutor();

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        if (state == null) receive(getIntent());
    }
    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        receive(intent);
    }
    public void pickMedia() {
        runOnUiThread(() -> {
            try {
                Intent pick = new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*")
                    .addCategory(Intent.CATEGORY_OPENABLE)
                    .putExtra(Intent.EXTRA_MIME_TYPES, new String[] {"image/*", "video/*"})
                    .putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
                startActivityForResult(pick, PICK_MEDIA);
            } catch (RuntimeException error) { event(null, error, true, false); }
        });
    }
    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != PICK_MEDIA) return;
        if (result != RESULT_OK || data == null) { event(null, null, true, false); return; }
        try { copyUris(data, false); }
        catch (RuntimeException error) { event(null, error, true, false); }
    }
    private void receive(Intent intent) {
        if (intent == null) return;
        try {
            String action = intent.getAction();
            if (Intent.ACTION_SEND.equals(action) || Intent.ACTION_SEND_MULTIPLE.equals(action)
                    || (Intent.ACTION_VIEW.equals(action) && intent.getData() != null
                        && "content".equals(intent.getData().getScheme()))) copyUris(intent, true);
        } catch (RuntimeException error) { event(null, error, true, false); }
    }

    private void copyUris(Intent intent, boolean open) {
        java.util.LinkedHashSet<Uri> uris = new java.util.LinkedHashSet<>();
        if (intent.getData() != null) uris.add(intent.getData());
        ClipData clip = intent.getClipData();
        if (clip != null) for (int i = 0; i < clip.getItemCount(); i++)
            if (clip.getItemAt(i).getUri() != null) uris.add(clip.getItemAt(i).getUri());
        if (Intent.ACTION_SEND_MULTIPLE.equals(intent.getAction())) {
            java.util.ArrayList<?> streams = intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
            if (streams != null) for (Object stream : streams)
                if (stream instanceof Uri) uris.add((Uri)stream);
        } else {
            android.os.Parcelable stream = intent.getParcelableExtra(Intent.EXTRA_STREAM);
            if (stream instanceof Uri) uris.add((Uri)stream);
        }
        copies.execute(() -> {
            for (Uri uri : uris) {
                try { event(copy(uri), null, false, open); }
                catch (IOException | RuntimeException error) { event(null, error, false, false); }
            }
            event(null, null, true, false);
        });
    }
    private File copy(Uri uri) throws IOException {
        if (!"content".equals(uri.getScheme())) throw new IOException("video.invalid_file");
        String name = "media";
        try (Cursor cursor = getContentResolver().query(uri,
                new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst() && !cursor.isNull(0)) name = cursor.getString(0);
        }
        name = new File(name).getName().replaceAll("[\\p{Cntrl}]", "_");
        int dot = name.lastIndexOf('.');
        String stem = dot > 0 ? name.substring(0, dot) : name;
        String suffix = dot > 0 ? name.substring(dot) : "";
        if (suffix.isEmpty()) {
            String mime = getContentResolver().getType(uri);
            String extension = mime == null ? null : MimeTypeMap.getSingleton().getExtensionFromMimeType(mime);
            suffix = extension == null ? ".mp4" : "." + extension;
        }
        if (stem.length() < 3) stem = "media-" + stem;
        stem = stem.substring(0, Math.min(stem.length(), 80));
        File root = getExternalFilesDir(null);
        if (root == null) root = getFilesDir();
        File folder = new File(root, "Documents/Imports");
        if (!folder.isDirectory() && !folder.mkdirs()) throw new IOException("video.import_failed");
        File target = File.createTempFile(stem + "-", suffix.toLowerCase(Locale.ROOT), folder);
        boolean success = false;
        try (InputStream input = getContentResolver().openInputStream(uri);
             OutputStream output = new FileOutputStream(target)) {
            if (input == null) throw new IOException("video.invalid_file");
            byte[] bytes = new byte[128 * 1024];
            int size;
            while ((size = input.read(bytes)) != -1) output.write(bytes, 0, size);
            success = true;
        } finally { if (!success) target.delete(); }
        return target;
    }
    private static void event(File file, Exception error, boolean done, boolean open) {
        try {
            JSONObject value = new JSONObject();
            if (file != null) value.put("path", file.getAbsolutePath());
            if (error != null) value.put("error", error.toString());
            value.put("done", done).put("open", open);
            imports.add(value);
        } catch (org.json.JSONException impossible) { throw new AssertionError(impossible); }
    }
    public String takeMediaImports() {
        JSONArray values = new JSONArray();
        JSONObject value;
        while ((value = imports.poll()) != null) values.put(value);
        return values.toString();
    }
    public void shareVideo(String path, String title) throws IOException {
        Uri uri = VideoProvider.share(this, new File(path));
        String mime = VideoProvider.mime(new File(path));
        Intent send = new Intent(Intent.ACTION_SEND).setType(mime)
            .putExtra(Intent.EXTRA_STREAM, uri)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        send.setClipData(ClipData.newRawUri("", uri));
        runOnUiThread(() -> {
            try { startActivity(Intent.createChooser(send, title)); }
            catch (RuntimeException error) { event(null, error, true, false); }
        });
    }
}
