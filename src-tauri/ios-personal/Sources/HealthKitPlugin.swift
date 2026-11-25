import Foundation
import HealthKit
import SwiftRs
import Tauri
import UIKit
import WebKit

private struct ReadBloodGlucoseArgs: Decodable {
    let days: Int?
}

/// Clean-room iOS Apple HealthKit Tauri v2 plugin for `moe.dabney.personal`.
///
/// Reads `HKQuantityTypeIdentifier.bloodGlucose` samples (including Abbott Lingo syncs
/// written to Apple Health) strictly locally on-device and serializes them into the
/// canonical `dabney.health_bridge.v1` JSON contract parsed by
/// `crates/ui/src/mental_health_cgm.rs`.
///
/// Never bundled in the zero-PHI public demo (`moe.dabney.demo`).
class HealthKitPlugin: Plugin {
    private let healthStore = HKHealthStore()

    private static let bridgeSchema = "dabney.health_bridge.v1"
    private static let defaultDays = 14
    private static let maxDays = 180
    private static let maxRecords = 60_000
    private static let bloodGlucoseIdentifier = "HKQuantityTypeIdentifierBloodGlucose"

    private static let mgDlUnit: HKUnit = {
        HKUnit.gramUnit(with: .milli).unitDivided(by: HKUnit.literUnit(with: .deci))
    }()

    private func bloodGlucoseType() -> HKQuantityType? {
        HKQuantityType.quantityType(forIdentifier: .bloodGlucose)
    }

    private func formatZoneOffset(for date: Date, metadata: [String: Any]?) -> String {
        let tz: TimeZone
        if let tzName = metadata?[HKMetadataKeyTimeZone] as? String,
           let parsed = TimeZone(identifier: tzName)
        {
            tz = parsed
        } else {
            tz = TimeZone.current
        }
        let totalSeconds = tz.secondsFromGMT(for: date)
        let sign = totalSeconds >= 0 ? "+" : "-"
        let absSeconds = abs(totalSeconds)
        let hours = absSeconds / 3600
        let minutes = (absSeconds % 3600) / 60
        return String(format: "%@%02d:%02d", sign, hours, minutes)
    }

    private func mealRelationLabel(metadata: [String: Any]?) -> String? {
        guard let raw = metadata?[HKMetadataKeyBloodGlucoseMealTime] as? NSNumber else {
            return nil
        }
        switch raw.intValue {
        case HKBloodGlucoseMealTime.preprandial.rawValue:
            return "before_meal"
        case HKBloodGlucoseMealTime.postprandial.rawValue:
            return "after_meal"
        default:
            return nil
        }
    }

    private func specimenSourceLabel(sourceName: String, bundleId: String) -> String {
        let combined = "\(sourceName) \(bundleId)".lowercased()
        let cgmTokens = ["lingo", "libre", "dexcom", "stelo", "medtronic", "eversense", "tandem", "omnipod"]
        if cgmTokens.contains(where: { combined.contains($0) }) {
            return "interstitial_fluid"
        }
        return "unknown"
    }

    @objc public func checkStatus(_ invoke: Invoke) {
        let packageId = Bundle.main.bundleIdentifier ?? "moe.dabney.personal"
        guard HKHealthStore.isHealthDataAvailable(), let glucoseType = bloodGlucoseType() else {
            invoke.resolve([
                "enabled": true,
                "platform": "ios_healthkit",
                "sdk_status": "unavailable",
                "has_glucose_permission": false,
                "package_id": packageId,
            ])
            return
        }

        // Apple HealthKit intentionally withholds whether the user granted or denied
        // read-only access for privacy. `getRequestStatusForAuthorization` tells us
        // whether the authorization sheet has already been presented (`.unnecessary`).
        healthStore.getRequestStatusForAuthorization(toShare: [], read: [glucoseType]) { status, _ in
            let alreadyPrompted = (status == .unnecessary)
            invoke.resolve([
                "enabled": true,
                "platform": "ios_healthkit",
                "sdk_status": "available",
                "has_glucose_permission": alreadyPrompted,
                "package_id": packageId,
            ])
        }
    }

    @objc public func requestGlucosePermission(_ invoke: Invoke) {
        guard HKHealthStore.isHealthDataAvailable(), let glucoseType = bloodGlucoseType() else {
            invoke.reject("Apple HealthKit is not available on this device.")
            return
        }

        healthStore.requestAuthorization(toShare: [], read: [glucoseType]) { success, error in
            if let error = error {
                invoke.reject("HealthKit authorization failed: \(error.localizedDescription)")
                return
            }
            invoke.resolve([
                "granted": success,
                "granted_permissions": success ? [Self.bloodGlucoseIdentifier] : [],
            ])
        }
    }

    @objc public func readBloodGlucose(_ invoke: Invoke) {
        guard HKHealthStore.isHealthDataAvailable(), let glucoseType = bloodGlucoseType() else {
            invoke.reject("Apple HealthKit is not available on this device.")
            return
        }

        let parsedDays: Int
        if let args = try? invoke.parseArgs(ReadBloodGlucoseArgs.self), let d = args.days {
            parsedDays = d
        } else {
            parsedDays = Self.defaultDays
        }
        let clampedDays = max(1, min(Self.maxDays, parsedDays))

        let endDate = Date()
        let startDate = Calendar.current.date(byAdding: .day, value: -clampedDays, to: endDate)
            ?? endDate.addingTimeInterval(-Double(clampedDays) * 86_400.0)

        let predicate = HKQuery.predicateForSamples(
            withStart: startDate,
            end: endDate,
            options: .strictStartDate
        )
        let sort = NSSortDescriptor(key: HKSampleSortIdentifierStartDate, ascending: true)

        let query = HKSampleQuery(
            sampleType: glucoseType,
            predicate: predicate,
            limit: Self.maxRecords,
            sortDescriptors: [sort]
        ) { [weak self] _, samples, error in
            guard let self = self else { return }
            if let error = error {
                invoke.reject("Failed to read HealthKit blood glucose samples: \(error.localizedDescription)")
                return
            }

            let isoFormatter = ISO8601DateFormatter()
            isoFormatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            isoFormatter.timeZone = TimeZone(secondsFromGMT: 0)

            let quantitySamples = (samples as? [HKQuantitySample]) ?? []
            var records: [[String: Any]] = []
            records.reserveCapacity(quantitySamples.count)
            var originOrder: [String] = []
            var seenOrigins = Set<String>()

            for sample in quantitySamples {
                let mgDl = sample.quantity.doubleValue(for: Self.mgDlUnit)
                if !mgDl.isFinite || mgDl <= 0 {
                    continue
                }
                let sourceName = sample.sourceRevision.source.name.trimmingCharacters(in: .whitespacesAndNewlines)
                let bundleId = sample.sourceRevision.source.bundleIdentifier.trimmingCharacters(in: .whitespacesAndNewlines)
                let origin = !sourceName.isEmpty ? sourceName : bundleId

                var item: [String: Any] = [
                    "time": isoFormatter.string(from: sample.startDate),
                    "zoneOffset": self.formatZoneOffset(for: sample.startDate, metadata: sample.metadata),
                    "levelMgDl": mgDl,
                    "specimenSource": self.specimenSourceLabel(sourceName: sourceName, bundleId: bundleId),
                ]
                if let meal = self.mealRelationLabel(metadata: sample.metadata) {
                    item["relationToMeal"] = meal
                }
                if !origin.isEmpty {
                    item["dataOrigin"] = origin
                    if seenOrigins.insert(origin).inserted {
                        originOrder.append(origin)
                    }
                }
                records.append(item)
            }

            let envelope: [String: Any] = [
                "schema": Self.bridgeSchema,
                "platform": "ios_healthkit",
                "exported_at": isoFormatter.string(from: endDate),
                "records": records,
            ]

            do {
                let data = try JSONSerialization.data(withJSONObject: envelope, options: [])
                guard let bridgeJson = String(data: data, encoding: .utf8) else {
                    invoke.reject("Failed to UTF-8 encode dabney.health_bridge.v1 JSON payload.")
                    return
                }
                invoke.resolve([
                    "bridge_json": bridgeJson,
                    "record_count": records.count,
                    "sources": originOrder,
                ])
            } catch {
                invoke.reject("Failed to serialize HealthKit JSON payload: \(error.localizedDescription)")
            }
        }

        healthStore.execute(query)
    }
}

@_cdecl("init_plugin_dabney_health_kit")
public func initPlugin() -> Plugin {
    return HealthKitPlugin()
}
