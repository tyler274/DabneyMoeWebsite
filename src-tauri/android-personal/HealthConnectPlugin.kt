package moe.dabney.personal

import android.app.Activity
import androidx.activity.result.ActivityResult
import androidx.health.connect.client.HealthConnectClient
import androidx.health.connect.client.PermissionController
import androidx.health.connect.client.permission.HealthPermission
import androidx.health.connect.client.records.BloodGlucoseRecord
import androidx.health.connect.client.request.ReadRecordsRequest
import androidx.health.connect.client.time.TimeRangeFilter
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.time.Instant
import java.time.ZoneId
import java.time.temporal.ChronoUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/**
 * Clean-room Android Google Health Connect bridge for `moe.dabney.personal`.
 *
 * Reads blood glucose records (including Abbott Lingo syncs into Health Connect)
 * strictly locally on-device and serializes them into the canonical
 * `dabney.health_bridge.v1` JSON contract parsed by `crates/ui/src/mental_health_cgm.rs`.
 *
 * Never bundled in the zero-PHI public Play Store demo (`moe.dabney.demo`).
 */
@TauriPlugin
class HealthConnectPlugin(private val activity: Activity) : Plugin(activity) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    companion object {
        private const val PROVIDER_PACKAGE = "com.google.android.apps.healthdata"
        private const val BRIDGE_SCHEMA = "dabney.health_bridge.v1"
        private const val DEFAULT_DAYS = 14
        private const val MAX_DAYS = 180
        private const val MAX_RECORDS = 60_000

        private val READ_GLUCOSE_PERMISSION: String =
            HealthPermission.getReadPermission(BloodGlucoseRecord::class)
        private val REQUIRED_PERMISSIONS: Set<String> = setOf(READ_GLUCOSE_PERMISSION)
    }

    private fun sdkStatusLabel(status: Int): String =
        when (status) {
            HealthConnectClient.SDK_AVAILABLE -> "available"
            HealthConnectClient.SDK_UNAVAILABLE_PROVIDER_UPDATE_REQUIRED -> "update_required"
            else -> "unavailable"
        }

    private fun mealRelationLabel(relation: Int): String? =
        when (relation) {
            BloodGlucoseRecord.RELATION_TO_MEAL_FASTING -> "fasting"
            BloodGlucoseRecord.RELATION_TO_MEAL_BEFORE_MEAL -> "before_meal"
            BloodGlucoseRecord.RELATION_TO_MEAL_AFTER_MEAL -> "after_meal"
            BloodGlucoseRecord.RELATION_TO_MEAL_GENERAL -> "general"
            else -> null
        }

    private fun specimenSourceLabel(source: Int): String =
        when (source) {
            BloodGlucoseRecord.SPECIMEN_SOURCE_INTERSTITIAL_FLUID -> "interstitial_fluid"
            BloodGlucoseRecord.SPECIMEN_SOURCE_CAPILLARY_BLOOD -> "capillary_blood"
            BloodGlucoseRecord.SPECIMEN_SOURCE_PLASMA -> "plasma"
            BloodGlucoseRecord.SPECIMEN_SOURCE_SERUM -> "serum"
            BloodGlucoseRecord.SPECIMEN_SOURCE_TEARS -> "tears"
            BloodGlucoseRecord.SPECIMEN_SOURCE_WHOLE_BLOOD -> "whole_blood"
            else -> "unknown"
        }

    @Command
    fun checkStatus(invoke: Invoke) {
        scope.launch {
            try {
                val statusInt = HealthConnectClient.getSdkStatus(activity, PROVIDER_PACKAGE)
                val statusLabel = sdkStatusLabel(statusInt)
                val granted =
                    if (statusInt == HealthConnectClient.SDK_AVAILABLE) {
                        val client = HealthConnectClient.getOrCreate(activity)
                        client.permissionController.getGrantedPermissions()
                            .containsAll(REQUIRED_PERMISSIONS)
                    } else {
                        false
                    }

                val res = JSObject()
                res.put("enabled", true)
                res.put("platform", "android_health_connect")
                res.put("sdk_status", statusLabel)
                res.put("has_glucose_permission", granted)
                res.put("package_id", activity.packageName)
                invoke.resolve(res)
            } catch (err: Exception) {
                invoke.reject("Failed to query Health Connect status: ${err.message ?: err.javaClass.simpleName}")
            }
        }
    }

    @Command
    fun requestGlucosePermission(invoke: Invoke) {
        scope.launch {
            try {
                val statusInt = HealthConnectClient.getSdkStatus(activity, PROVIDER_PACKAGE)
                if (statusInt != HealthConnectClient.SDK_AVAILABLE) {
                    val label = sdkStatusLabel(statusInt)
                    invoke.reject("Health Connect is not available on this device (status: $label).")
                    return@launch
                }

                val client = HealthConnectClient.getOrCreate(activity)
                val alreadyGranted = client.permissionController.getGrantedPermissions()
                if (alreadyGranted.containsAll(REQUIRED_PERMISSIONS)) {
                    val arr = JSArray()
                    alreadyGranted.sorted().forEach { arr.put(it) }
                    val res = JSObject()
                    res.put("granted", true)
                    res.put("granted_permissions", arr)
                    invoke.resolve(res)
                    return@launch
                }

                val contract = PermissionController.createRequestPermissionResultContract()
                val intent = contract.createIntent(activity, REQUIRED_PERMISSIONS)
                activity.runOnUiThread {
                    startActivityForResult(invoke, intent, "onPermissionResult")
                }
            } catch (err: Exception) {
                invoke.reject("Failed to launch Health Connect permission sheet: ${err.message ?: err.javaClass.simpleName}")
            }
        }
    }

    @ActivityCallback
    private fun onPermissionResult(invoke: Invoke, result: ActivityResult) {
        try {
            val contract = PermissionController.createRequestPermissionResultContract()
            val grantedSet = contract.parseResult(result.resultCode, result.data)
            val hasGlucose = grantedSet.containsAll(REQUIRED_PERMISSIONS)
            val arr = JSArray()
            grantedSet.sorted().forEach { arr.put(it) }
            val res = JSObject()
            res.put("granted", hasGlucose)
            res.put("granted_permissions", arr)
            invoke.resolve(res)
        } catch (err: Exception) {
            invoke.reject("Failed to process Health Connect permission result: ${err.message ?: err.javaClass.simpleName}")
        }
    }

    @Command
    fun readBloodGlucose(invoke: Invoke) {
        val requestedDays = invoke.getArgs().getInteger("days", DEFAULT_DAYS)
        val days = requestedDays.coerceIn(1, MAX_DAYS)

        scope.launch {
            try {
                val statusInt = HealthConnectClient.getSdkStatus(activity, PROVIDER_PACKAGE)
                if (statusInt != HealthConnectClient.SDK_AVAILABLE) {
                    invoke.reject("Health Connect is not available (status: ${sdkStatusLabel(statusInt)}).")
                    return@launch
                }

                val client = HealthConnectClient.getOrCreate(activity)
                val granted = client.permissionController.getGrantedPermissions()
                if (!granted.containsAll(REQUIRED_PERMISSIONS)) {
                    invoke.reject("Blood glucose read permission has not been granted in Health Connect.")
                    return@launch
                }

                val end = Instant.now()
                val start = end.minus(days.toLong(), ChronoUnit.DAYS)

                val recordsJson = JSONArray()
                val origins = LinkedHashSet<String>()
                var pageToken: String? = null
                var count = 0

                do {
                    val response =
                        client.readRecords(
                            ReadRecordsRequest(
                                recordType = BloodGlucoseRecord::class,
                                timeRangeFilter = TimeRangeFilter.between(start, end),
                                pageToken = pageToken,
                            ),
                        )
                    for (rec in response.records) {
                        if (count >= MAX_RECORDS) break
                        val item = JSONObject()
                        item.put("time", rec.time.toString())
                        val resolvedOffset = rec.zoneOffset ?: ZoneId.systemDefault().rules.getOffset(rec.time)
                        item.put("zoneOffset", resolvedOffset.id)
                        item.put("levelMgDl", rec.level.inMilligramsPerDeciliter)
                        mealRelationLabel(rec.relationToMeal)?.let { item.put("relationToMeal", it) }
                        item.put("specimenSource", specimenSourceLabel(rec.specimenSource))
                        val origin = rec.metadata.dataOrigin.packageName
                        if (origin.isNotBlank()) {
                            item.put("dataOrigin", origin)
                            origins.add(origin)
                        }
                        recordsJson.put(item)
                        count += 1
                    }
                    pageToken = if (count >= MAX_RECORDS) null else response.pageToken
                } while (!pageToken.isNullOrEmpty())

                val envelope = JSONObject()
                envelope.put("schema", BRIDGE_SCHEMA)
                envelope.put("platform", "android_health_connect")
                envelope.put("exported_at", end.toString())
                envelope.put("records", recordsJson)

                val sourcesArr = JSArray()
                origins.forEach { sourcesArr.put(it) }

                val res = JSObject()
                res.put("bridge_json", envelope.toString())
                res.put("record_count", count)
                res.put("sources", sourcesArr)
                invoke.resolve(res)
            } catch (err: Exception) {
                invoke.reject("Failed to read blood glucose from Health Connect: ${err.message ?: err.javaClass.simpleName}")
            }
        }
    }
}
