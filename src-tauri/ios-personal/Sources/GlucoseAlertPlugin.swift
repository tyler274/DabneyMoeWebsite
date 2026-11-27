import Foundation
import SwiftRs
import Tauri
import UIKit
import UserNotifications
import WebKit

private struct SendGlucoseNotificationArgs: Decodable {
    let title: String?
    let body: String?
    let severity: String?
    let tag: String?
}

/// Clean-room iOS `UserNotifications` Tauri v2 plugin for `moe.dabney.personal`.
///
/// Delivers opt-in local glucose range alert notifications via `UNUserNotificationCenter`
/// strictly on-device without remote push or cloud telemetry.
/// Never logs notification titles, bodies, or glucose values.
/// Never bundled in the zero-PHI public demo (`moe.dabney.demo`).
class GlucoseAlertPlugin: Plugin {
    private static let platformId = "ios_user_notifications"
    private static let standardChannelId = "dabney_glucose_alerts"
    private static let urgentChannelId = "dabney_glucose_urgent"
    private static let defaultTag = "dabney_glucose_alert"
    private static let maxTitleChars = 160
    private static let maxBodyChars = 512
    private static let maxTagChars = 96

    private func mapAuthorizationStatus(_ status: UNAuthorizationStatus) -> (Bool, String) {
        switch status {
        case .authorized:
            return (true, "granted")
        case .provisional:
            return (true, "provisional")
        case .ephemeral:
            return (true, "ephemeral")
        case .denied:
            return (false, "denied")
        case .notDetermined:
            return (false, "prompt")
        @unknown default:
            return (false, "unknown")
        }
    }

    private func isUrgentSeverity(_ severity: String) -> Bool {
        let lower = severity.lowercased()
        return lower == "urgent" || lower == "critical"
    }

    /// Stable non-negative 32-bit FNV-1a identifier (avoids per-process Swift hash randomization).
    private func stableNotificationId(for tag: String) -> Int32 {
        var hash: UInt32 = 2_166_136_261
        for byte in tag.utf8 {
            hash ^= UInt32(byte)
            hash = hash &* 16_777_619
        }
        return Int32(hash & 0x7fff_ffff)
    }

    @objc public func checkStatus(_ invoke: Invoke) {
        UNUserNotificationCenter.current().getNotificationSettings { settings in
            let (granted, status) = self.mapAuthorizationStatus(settings.authorizationStatus)
            invoke.resolve([
                "enabled": true,
                "platform": Self.platformId,
                "permission_granted": granted,
                "permission_status": status,
                "channel_id": Self.standardChannelId,
            ])
        }
    }

    @objc public func requestPermission(_ invoke: Invoke) {
        let center = UNUserNotificationCenter.current()
        center.requestAuthorization(options: [.alert, .sound, .badge]) { granted, error in
            if let error = error {
                invoke.reject("Failed to request iOS notification authorization: \(error.localizedDescription)")
                return
            }
            center.getNotificationSettings { settings in
                let (effectiveGranted, status) = self.mapAuthorizationStatus(settings.authorizationStatus)
                invoke.resolve([
                    "granted": granted || effectiveGranted,
                    "status": status,
                ])
            }
        }
    }

    @objc public func sendNotification(_ invoke: Invoke) {
        let args: SendGlucoseNotificationArgs
        do {
            args = try invoke.parseArgs(SendGlucoseNotificationArgs.self)
        } catch {
            invoke.reject("Invalid alert notification payload.")
            return
        }

        let title = (args.title ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let body = (args.body ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let severity = (args.severity ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        let rawTag = (args.tag ?? "").trimmingCharacters(in: .whitespacesAndNewlines)

        if title.isEmpty || title.count > Self.maxTitleChars {
            invoke.reject("Invalid alert notification title.")
            return
        }
        if body.isEmpty || body.count > Self.maxBodyChars {
            invoke.reject("Invalid alert notification body.")
            return
        }
        if rawTag.count > Self.maxTagChars {
            invoke.reject("Invalid alert notification tag.")
            return
        }

        let center = UNUserNotificationCenter.current()
        center.getNotificationSettings { settings in
            let (granted, status) = self.mapAuthorizationStatus(settings.authorizationStatus)
            if !granted {
                invoke.resolve([
                    "delivered": false,
                    "notification_id": 0,
                    "detail": "Notification permission not granted (\(status)).",
                ])
                return
            }

            let urgent = self.isUrgentSeverity(severity)
            let channelId = urgent ? Self.urgentChannelId : Self.standardChannelId
            let effectiveTag = rawTag.isEmpty ? Self.defaultTag : rawTag
            let notificationId = self.stableNotificationId(for: effectiveTag)

            let content = UNMutableNotificationContent()
            content.title = title
            content.body = body
            content.sound = .default
            content.threadIdentifier = channelId
            if #available(iOS 15.0, *) {
                content.interruptionLevel = urgent ? .timeSensitive : .active
            }

            let request = UNNotificationRequest(
                identifier: effectiveTag,
                content: content,
                trigger: nil
            )
            center.add(request) { error in
                if let error = error {
                    invoke.reject("Failed to schedule iOS alert notification: \(error.localizedDescription)")
                    return
                }
                invoke.resolve([
                    "delivered": true,
                    "notification_id": notificationId,
                    "detail": "Scheduled on \(channelId)",
                ])
            }
        }
    }
}

@_cdecl("init_plugin_dabney_glucose_alerts")
public func initGlucoseAlertPlugin() -> Plugin {
    return GlucoseAlertPlugin()
}
