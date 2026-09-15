package com.infrawrench.schist;

import android.app.job.JobParameters;
import android.app.job.JobService;
import android.app.Activity;
import android.content.Context;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import java.security.KeyStore;
import java.security.Key;
import java.util.Arrays;
import javax.crypto.Cipher;
import javax.crypto.spec.GCMParameterSpec;

/**
 * The camera-roll backup's background wake: the one piece of Java in
 * Schist. JobScheduler starts this every so often while the rule is on
 * (crates/camera-sync/src/android.rs schedules it), and
 * it hands straight to the Rust engine in the app's own library, which
 * runs headless -- no activity, no gpui -- and returns when the cloud
 * folder is up to date. tools/android-build.sh compiles this with javac
 * and d8 into the APK's classes.dex.
 */
public class CameraSyncJob extends JobService {
    static {
        System.loadLibrary("schist_app");
    }

    /** Runs the backup to completion; false asks for a retry. */
    private static native boolean runSync(Context context);

    /** Stops a run in progress; it returns from runSync soon after. */
    private static native void stopSync();
    private static native void prepareSync();

    private JobParameters active;
    private final Handler main = new Handler(Looper.getMainLooper());

    // gpui's Rust thread is separate from Android's activity thread.
    public static void requestMediaPermission(Activity activity) {
        activity.runOnUiThread(() -> {
            String[] permissions = Build.VERSION.SDK_INT >= 34
                ? new String[] {"android.permission.READ_MEDIA_IMAGES",
                    "android.permission.READ_MEDIA_VISUAL_USER_SELECTED"}
                : new String[] {Build.VERSION.SDK_INT >= 33
                    ? "android.permission.READ_MEDIA_IMAGES"
                    : "android.permission.READ_EXTERNAL_STORAGE"};
            activity.requestPermissions(permissions, 1);
        });
    }

    // Same key and envelope as gpui's Android credential store. The key
    // stays in AndroidKeyStore; refreshed logins are encrypted here too.
    private static byte[] crypt(boolean encrypt, byte[] input) throws Exception {
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        Key key = store.getKey("gpui.credentials", null);
        if (key == null) throw new IllegalStateException("No stored cloud login");
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        if (encrypt) {
            cipher.init(Cipher.ENCRYPT_MODE, key);
            byte[] iv = cipher.getIV();
            byte[] encrypted = cipher.doFinal(input);
            byte[] output = new byte[1 + iv.length + encrypted.length];
            output[0] = (byte) iv.length;
            System.arraycopy(iv, 0, output, 1, iv.length);
            System.arraycopy(encrypted, 0, output, 1 + iv.length, encrypted.length);
            return output;
        }
        if (input.length < 2) throw new IllegalArgumentException("Truncated credentials");
        int ivLength = input[0] & 0xff;
        if (ivLength == 0 || input.length < 1 + ivLength + 16)
            throw new IllegalArgumentException("Truncated credentials");
        cipher.init(Cipher.DECRYPT_MODE, key,
            new GCMParameterSpec(128, Arrays.copyOfRange(input, 1, 1 + ivLength)));
        return cipher.doFinal(input, 1 + ivLength, input.length - 1 - ivLength);
    }

    @Override
    public boolean onStartJob(final JobParameters params) {
        active = params;
        prepareSync();
        final Context context = getApplicationContext();
        Thread worker = new Thread(() -> {
            boolean ok = false;
            try {
                ok = runSync(context);
            } catch (RuntimeException error) {
                Log.e("SchistCameraSync", "Background backup failed", error);
            } finally {
                final boolean success = ok;
                main.post(() -> {
                    if (active == params) {
                        active = null;
                        jobFinished(params, !success);
                    }
                });
            }
        }, "schist-camera-sync");
        worker.start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters params) {
        active = null;
        stopSync();
        return true;
    }
}
