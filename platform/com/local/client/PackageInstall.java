package com.local.client;

import android.app.PendingIntent;
import android.app.Activity;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageInstaller;
import android.os.Build;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.LinkedHashMap;
import java.util.Map;

/** Installs one package through a package-installer session. */
public final class PackageInstall {
    private static final String RESULT_ACTION = "com.local.client.PACKAGE_INSTALLED";
    private static final String SESSION_EXTRA = "rllm_install_session";
    private static final String RESULT_PREFIX = "result.";
    private static final String CONFIRMATION_PREFIX = "confirmation.";
    // The receiver and activity lifecycle both call these fields on the main thread.
    private static final Map<Integer, Intent> confirmations = new LinkedHashMap<>();
    private static com.local.client.Activity foreground;

    private PackageInstall() {}

    public static int install(Context context, byte[] apk, String packageName) throws Exception {
        if (apk == null || apk.length == 0) {
            throw new IllegalArgumentException("APK bytes are required");
        }
        return install(context, new java.io.ByteArrayInputStream(apk), packageName);
    }

    public static int install(Context context, InputStream apk, String packageName) throws Exception {
        return install(context, apk, packageName, "");
    }

    static int install(Context context, InputStream apk, String packageName, String chat) throws Exception {
        if (context == null || apk == null) {
            throw new IllegalArgumentException("An activity and APK stream are required");
        }
        if (!(context instanceof Activity)) {
            throw new IllegalArgumentException("Package installation requires an activity context");
        }
        PackageInstaller installer = context.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        if (packageName != null && packageName.length() > 0) {
            params.setAppPackageName(packageName);
        }
        int sessionId = installer.createSession(params);
        try {
            PackageInstaller.Session session = installer.openSession(sessionId);
            try {
                try (OutputStream out = session.openWrite("base.apk", 0, -1)) {
                    byte[] buffer = new byte[65536];
                    long total = 0;
                    int read;
                    while ((read = apk.read(buffer)) != -1) {
                        if (read > 0) {
                            total = Math.addExact(total, read);
                            out.write(buffer, 0, read);
                        }
                    }
                    if (total == 0) {
                        throw new IOException("APK stream is empty");
                    }
                    session.fsync(out);
                }
                if (!sessions(context).edit().putString(Integer.toString(sessionId), chat).commit()) {
                    throw new IOException("Could not save the Android installation callback");
                }
                session.commit(callback(context, sessionId, 0).getIntentSender());
                return sessionId;
            } finally {
                session.close();
            }
        } catch (Exception failure) {
            forget(context, sessionId);
            cancelCallback(context, sessionId);
            try {
                installer.abandonSession(sessionId);
            } catch (Exception ignored) {
            }
            throw failure;
        }
    }

    static void resume(com.local.client.Activity activity) {
        foreground = activity;
        deliver(activity);
    }

    static void pause(com.local.client.Activity activity) {
        if (foreground == activity) foreground = null;
    }

    static void deliver(com.local.client.Activity activity) {
        if (foreground != activity || !activity.apkResultsReady()) return;
        SharedPreferences sessions = sessions(activity);
        for (String result : sessions.getAll().keySet()) {
            if (!result.startsWith(RESULT_PREFIX)) continue;
            String key = result.substring(RESULT_PREFIX.length());
            activity.apkInstallResult(sessions.getString(key, ""), sessions.getString(result, ""),
                sessions.getBoolean(key + ".failed", false));
            forget(activity, Integer.parseInt(key));
        }
        // A confirmation may have reached the receiver before the activity was
        // resumed. Keep the Android intent available if that process is recreated.
        for (String entry : sessions.getAll().keySet()) {
            if (!entry.startsWith(CONFIRMATION_PREFIX)) continue;
            int sessionId = Integer.parseInt(entry.substring(CONFIRMATION_PREFIX.length()));
            try {
                if (!confirmations.containsKey(sessionId)) {
                    confirmations.put(sessionId, Intent.parseUri(sessions.getString(entry, ""), Intent.URI_ALLOW_UNSAFE));
                }
            } catch (java.net.URISyntaxException failure) {
                confirmationFailed(activity, sessionId, failure);
            }
        }
        // Launch one confirmation at a time, since starting it pauses this activity.
        while (!confirmations.isEmpty()) {
            int sessionId = confirmations.keySet().iterator().next();
            Intent confirmation = confirmations.remove(sessionId);
            try {
                activity.startActivity(confirmation);
                sessions.edit().remove(CONFIRMATION_PREFIX + sessionId).apply();
                break;
            } catch (RuntimeException failure) {
                confirmationFailed(activity, sessionId, failure);
            }
        }
    }

    private static void confirmationFailed(com.local.client.Activity activity, int sessionId, Exception failure) {
        String chat = sessions(activity).getString(Integer.toString(sessionId), "");
        forget(activity, sessionId);
        cancelCallback(activity, sessionId);
        abandon(activity, sessionId);
        activity.apkInstallFailed(chat, failure);
    }

    private static void abandon(Context context, int sessionId) {
        try {
            context.getPackageManager().getPackageInstaller().abandonSession(sessionId);
        } catch (RuntimeException ignored) {}
    }

    /** Receives results even while the confirmation or another app is foregrounded. */
    public static final class ResultReceiver extends BroadcastReceiver {
        @Override public void onReceive(Context context, Intent result) {
            if (!RESULT_ACTION.equals(result.getAction())) return;
            int sessionId = result.getIntExtra(SESSION_EXTRA, -1);
            String key = Integer.toString(sessionId);
            SharedPreferences sessions = sessions(context);
            if (!sessions.contains(key)) return;
            int status = result.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
            if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
                Intent confirmation = result.getParcelableExtra(Intent.EXTRA_INTENT);
                if (confirmation != null) {
                    confirmations.put(sessionId, confirmation);
                    sessions.edit().putString(CONFIRMATION_PREFIX + key, confirmation.toUri(Intent.URI_INTENT_SCHEME)).apply();
                } else {
                    saveResult(context, sessionId, "APK installation failed: Android did not supply an installation confirmation", true);
                    abandon(context, sessionId);
                }
            } else if (status == PackageInstaller.STATUS_SUCCESS) {
                String name = result.getStringExtra(PackageInstaller.EXTRA_PACKAGE_NAME);
                saveResult(context, sessionId, "Installed " + (name == null || name.isEmpty() ? "APK" : name), false);
            } else {
                String reason = result.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
                if (reason == null || reason.isEmpty()) reason = "Android installation status " + status;
                saveResult(context, sessionId, "APK installation failed: " + reason, true);
            }
            if (foreground != null) deliver(foreground);
        }
    }

    private static void saveResult(Context context, int sessionId, String message, boolean failed) {
        confirmations.remove(sessionId);
        String key = Integer.toString(sessionId);
        sessions(context).edit().remove(CONFIRMATION_PREFIX + key).putString(RESULT_PREFIX + key, message)
            .putBoolean(key + ".failed", failed).apply();
        cancelCallback(context, sessionId);
    }

    private static void cancelCallback(Context context, int sessionId) {
        PendingIntent callback = callback(context, sessionId, PendingIntent.FLAG_NO_CREATE);
        if (callback != null) callback.cancel();
    }

    private static SharedPreferences sessions(Context context) {
        return context.getSharedPreferences("package-install", Context.MODE_PRIVATE);
    }

    private static void forget(Context context, int sessionId) {
        String key = Integer.toString(sessionId);
        sessions(context).edit().remove(key).remove(RESULT_PREFIX + key).remove(CONFIRMATION_PREFIX + key)
            .remove(key + ".failed").apply();
    }

    private static PendingIntent callback(Context context, int sessionId, int flags) {
        Intent intent = new Intent(context, ResultReceiver.class);
        intent.setAction(RESULT_ACTION);
        intent.putExtra(SESSION_EXTRA, sessionId);
        if (Build.VERSION.SDK_INT >= 31) flags |= PendingIntent.FLAG_MUTABLE;
        return PendingIntent.getBroadcast(context, sessionId, intent, flags);
    }
}
