package moe.dabney.personal

import android.Manifest
import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.ActivityCompat
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/**
 * Clean-room Android local notification bridge for `moe.dabney.personal`.
 *
 * Uses built-in Android `NotificationManager` + `NotificationChannel` + `POST_NOTIFICATIONS`
 * (zero extra Maven dependencies) with `Notification.VISIBILITY_PRIVATE` so sensitive glucose
 * range alert details stay hidden on the lock screen.
 *
 * Never logs notification titles, bodies, or glucose values to logcat.
 * Never bundled in the zero-PHI public Play Store demo (`moe.dabney.demo`).
 */
@TauriPlugin(
    permissions = [
        Permission(
            strings = [Manifest.permission.POST_NOTIFICATIONS],
            alias = "notifications",
        ),
    ],
)
class GlucoseAlertPlugin(private val activity: Activity) : Plugin(activity) {
    companion object {
        private const val PLATFORM_ID = "android_notification_manager"
        private const val NOTIFICATIONS_ALIAS = "notifications"
        private const val CHANNEL_STANDARD = "dabney_glucose_alerts"
        private const val CHANNEL_URGENT = "dabney_glucose_urgent"
        private const val DEFAULT_TAG = "dabney_glucose_alert"
        private const val MAX_TITLE_CHARS = 160
        private const val MAX_BODY_CHARS = 512
        private const val MAX_TAG_CHARS = 96
    }

    private fun notificationManager(): NotificationManager? =
        activity.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager

    private fun ensureNotificationChannels(manager: NotificationManager) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return
        }
        val standardChannel =
            NotificationChannel(
                CHANNEL_STANDARD,
                "Glucose Range Alerts",
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = "Opt-in on-device glucose range and trend alerts"
                lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            }
        val urgentChannel =
            NotificationChannel(
                CHANNEL_URGENT,
                "Urgent Glucose Alerts",
                NotificationManager.IMPORTANCE_HIGH,
            ).apply {
                description = "Opt-in urgent on-device low or high glucose alerts"
                lockscreenVisibility = Notification.VISIBILITY_PRIVATE
            }
        manager.createNotificationChannel(standardChannel)
        manager.createNotificationChannel(urgentChannel)
    }

    private fun evaluatePermission(manager: NotificationManager?): Pair<Boolean, String> {
        if (manager == null) {
            return false to "unavailable"
        }
        val systemEnabled = manager.areNotificationsEnabled()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val runtimeGranted =
                ActivityCompat.checkSelfPermission(
                    activity,
                    Manifest.permission.POST_NOTIFICATIONS,
                ) == PackageManager.PERMISSION_GRANTED
            if (runtimeGranted) {
                return if (systemEnabled) {
                    true to "granted"
                } else {
                    false to "disabled_in_settings"
                }
            }
            val cached = getPermissionState(NOTIFICATIONS_ALIAS)
            val label =
                when (cached) {
                    PermissionState.DENIED -> "denied"
                    PermissionState.PROMPT_WITH_RATIONALE -> "prompt-with-rationale"
                    else -> "prompt"
                }
            return false to label
        }
        return if (systemEnabled) {
            true to "granted"
        } else {
            false to "disabled_in_settings"
        }
    }

    private fun isUrgentSeverity(severity: String): Boolean =
        severity.equals("urgent", ignoreCase = true) ||
            severity.equals("critical", ignoreCase = true)

    @Command
    fun checkStatus(invoke: Invoke) {
        try {
            val manager = notificationManager()
            if (manager != null) {
                ensureNotificationChannels(manager)
            }
            val (granted, status) = evaluatePermission(manager)
            val res = JSObject()
            res.put("enabled", true)
            res.put("platform", PLATFORM_ID)
            res.put("permission_granted", granted)
            res.put("permission_status", status)
            res.put("channel_id", CHANNEL_STANDARD)
            invoke.resolve(res)
        } catch (err: Exception) {
            invoke.reject("Failed to query notification status: ${err.javaClass.simpleName}")
        }
    }

    @Command
    fun requestPermission(invoke: Invoke) {
        try {
            val manager = notificationManager()
            if (manager != null) {
                ensureNotificationChannels(manager)
            }
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
                val (granted, status) = evaluatePermission(manager)
                val res = JSObject()
                res.put("granted", granted)
                res.put("status", status)
                invoke.resolve(res)
                return
            }

            val alreadyGranted =
                ActivityCompat.checkSelfPermission(
                    activity,
                    Manifest.permission.POST_NOTIFICATIONS,
                ) == PackageManager.PERMISSION_GRANTED
            if (alreadyGranted) {
                val (granted, status) = evaluatePermission(manager)
                val res = JSObject()
                res.put("granted", granted)
                res.put("status", status)
                invoke.resolve(res)
                return
            }

            activity.runOnUiThread {
                requestPermissionForAlias(
                    NOTIFICATIONS_ALIAS,
                    invoke,
                    "onNotificationPermissionResult",
                )
            }
        } catch (err: Exception) {
            invoke.reject("Failed to request notification permission: ${err.javaClass.simpleName}")
        }
    }

    @PermissionCallback
    private fun onNotificationPermissionResult(invoke: Invoke) {
        try {
            val (granted, status) = evaluatePermission(notificationManager())
            val res = JSObject()
            res.put("granted", granted)
            res.put("status", status)
            invoke.resolve(res)
        } catch (err: Exception) {
            invoke.reject("Failed to process notification permission result: ${err.javaClass.simpleName}")
        }
    }

    @Command
    fun sendNotification(invoke: Invoke) {
        try {
            val args = invoke.getArgs()
            val title = args.getString("title")?.trim().orEmpty()
            val body = args.getString("body")?.trim().orEmpty()
            val severity = args.getString("severity")?.trim().orEmpty()
            val rawTag = args.getString("tag")?.trim().orEmpty()

            if (title.isEmpty() || title.length > MAX_TITLE_CHARS) {
                invoke.reject("Invalid alert notification title.")
                return
            }
            if (body.isEmpty() || body.length > MAX_BODY_CHARS) {
                invoke.reject("Invalid alert notification body.")
                return
            }
            if (rawTag.length > MAX_TAG_CHARS) {
                invoke.reject("Invalid alert notification tag.")
                return
            }

            val manager = notificationManager()
            if (manager == null) {
                val res = JSObject()
                res.put("delivered", false)
                res.put("notification_id", 0)
                res.put("detail", "Android NotificationManager service unavailable.")
                invoke.resolve(res)
                return
            }

            val (granted, status) = evaluatePermission(manager)
            if (!granted) {
                val res = JSObject()
                res.put("delivered", false)
                res.put("notification_id", 0)
                res.put("detail", "Notification permission not granted ($status).")
                invoke.resolve(res)
                return
            }

            ensureNotificationChannels(manager)

            val urgent = isUrgentSeverity(severity)
            val channelId = if (urgent) CHANNEL_URGENT else CHANNEL_STANDARD
            val effectiveTag = if (rawTag.isNotEmpty()) rawTag else DEFAULT_TAG
            val notificationId = effectiveTag.hashCode() and 0x7fffffff

            val launchIntent =
                activity.packageManager.getLaunchIntentForPackage(activity.packageName)?.apply {
                    addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                }
            val pendingIntent =
                launchIntent?.let { intent ->
                    PendingIntent.getActivity(
                        activity,
                        notificationId,
                        intent,
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                    )
                }

            val builder =
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                    Notification.Builder(activity, channelId)
                } else {
                    @Suppress("DEPRECATION")
                    Notification.Builder(activity)
                }

            builder
                .setSmallIcon(activity.applicationInfo.icon)
                .setContentTitle(title)
                .setContentText(body)
                .setStyle(Notification.BigTextStyle().bigText(body))
                .setVisibility(Notification.VISIBILITY_PRIVATE)
                .setAutoCancel(true)
                .setCategory(if (urgent) Notification.CATEGORY_ALARM else Notification.CATEGORY_REMINDER)

            if (pendingIntent != null) {
                builder.setContentIntent(pendingIntent)
            }

            manager.notify(notificationId, builder.build())

            val res = JSObject()
            res.put("delivered", true)
            res.put("notification_id", notificationId)
            res.put("detail", "Posted to $channelId")
            invoke.resolve(res)
        } catch (err: SecurityException) {
            val res = JSObject()
            res.put("delivered", false)
            res.put("notification_id", 0)
            res.put("detail", "Notification permission denied by system.")
            invoke.resolve(res)
        } catch (err: Exception) {
            invoke.reject("Failed to post alert notification: ${err.javaClass.simpleName}")
        }
    }
}
