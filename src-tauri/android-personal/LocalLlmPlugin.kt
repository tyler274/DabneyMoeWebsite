package moe.dabney.personal

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.mlkit.genai.common.DownloadStatus
import com.google.mlkit.genai.common.FeatureStatus
import com.google.mlkit.genai.prompt.GenerateContentRequest
import com.google.mlkit.genai.prompt.Generation
import com.google.mlkit.genai.prompt.GenerativeModel
import com.google.mlkit.genai.prompt.TextPart
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/**
 * Clean-room on-device LLM bridge for `moe.dabney.personal` using Google ML Kit GenAI Prompt API
 * backed by Android AICore (Gemini Nano hardware NPU acceleration, zero multi-GB weights in APK).
 *
 * Gracefully reports `availability = "unavailable"` on emulators and non-AICore devices so the UI
 * falls back cleanly to deterministic grounded Rust synthesis. Never logs prompt or output text.
 */
@TauriPlugin
class LocalLlmPlugin(private val activity: Activity) : Plugin(activity) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    @Volatile
    private var cachedModel: GenerativeModel? = null

    companion object {
        private const val AICORE_PACKAGE = "com.google.android.aicore"
        private const val BACKEND_ID = "android_aicore_gemini_nano"
        private const val MODEL_DISPLAY_NAME = "Gemini Nano (Android AICore)"
        private const val MAX_PROMPT_CHARS = 12_000
        private const val MIN_OUTPUT_TOKENS = 64
        private const val DEFAULT_OUTPUT_TOKENS = 512
        private const val MAX_OUTPUT_TOKENS = 1024
    }

    private fun getOrCreateModel(): GenerativeModel {
        cachedModel?.let { return it }
        synchronized(this) {
            cachedModel?.let { return it }
            val created = Generation.getClient()
            cachedModel = created
            return created
        }
    }

    private fun isAicoreInstalled(): Boolean =
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                activity.packageManager.getPackageInfo(
                    AICORE_PACKAGE,
                    PackageManager.PackageInfoFlags.of(0),
                )
            } else {
                @Suppress("DEPRECATION")
                activity.packageManager.getPackageInfo(AICORE_PACKAGE, 0)
            }
            true
        } catch (_: Exception) {
            false
        }

    private fun totalRamMb(): Long {
        val am = activity.getSystemService(Context.ACTIVITY_SERVICE) as? ActivityManager ?: return 0L
        val memInfo = ActivityManager.MemoryInfo()
        am.getMemoryInfo(memInfo)
        return memInfo.totalMem / (1024L * 1024L)
    }

    private fun deviceSummary(ramMb: Long): String {
        val model = "${Build.MANUFACTURER} ${Build.MODEL}".trim()
        val soc =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && Build.SOC_MODEL.isNotBlank()) {
                Build.SOC_MODEL
            } else {
                Build.HARDWARE
            }
        val gb = if (ramMb > 0) String.format("%.1f GB RAM", ramMb / 1024.0) else "RAM unknown"
        return "$model · $soc · $gb · API ${Build.VERSION.SDK_INT}"
    }

    private fun statusString(status: Int): Pair<String, String> =
        when (status) {
            FeatureStatus.AVAILABLE ->
                "available" to "On-device Gemini Nano is ready via Android AICore (100% local NPU inference)."
            FeatureStatus.DOWNLOADABLE ->
                "downloadable" to "Gemini Nano is supported on this device and can be downloaded via Android AICore."
            FeatureStatus.DOWNLOADING ->
                "downloading" to "Android AICore is currently downloading Gemini Nano in the background."
            else ->
                "unavailable" to "Android AICore / Gemini Nano is unavailable on this device; grounded Rust synthesis is active."
        }

    @Command
    fun checkStatus(invoke: Invoke) {
        scope.launch {
            val ramMb = totalRamMb()
            val summary = deviceSummary(ramMb)
            val aicore = isAicoreInstalled()

            var rawStatusInt: Int? = null
            var probeErrClass: String? = null
            val (availability, detail) =
                try {
                    val statusInt = getOrCreateModel().checkStatus()
                    rawStatusInt = statusInt
                    statusString(statusInt)
                } catch (err: Throwable) {
                    probeErrClass = err.javaClass.simpleName
                    "unavailable" to
                        "On-device AICore probe unavailable (${err.javaClass.simpleName}); grounded Rust synthesis is active."
                }

            Log.i(
                "LocalLlmPlugin",
                "checkStatus: availability=$availability rawStatus=$rawStatusInt err=$probeErrClass aicore=$aicore api=${Build.VERSION.SDK_INT}",
            )

            val res = JSObject()
            res.put("enabled", true)
            res.put("backend", BACKEND_ID)
            res.put("model_name", MODEL_DISPLAY_NAME)
            res.put("availability", availability)
            res.put("device_summary", summary)
            res.put("total_ram_mb", ramMb)
            res.put("aicore_installed", aicore)
            res.put("detail", detail)
            invoke.resolve(res)
        }
    }

    @Command
    fun prepareModel(invoke: Invoke) {
        scope.launch {
            try {
                val model = getOrCreateModel()
                when (val current = model.checkStatus()) {
                    FeatureStatus.AVAILABLE -> {
                        val res = JSObject()
                        res.put("ready", true)
                        res.put("status", "available")
                        res.put("detail", "Gemini Nano is already ready on-device via Android AICore.")
                        invoke.resolve(res)
                    }
                    FeatureStatus.DOWNLOADABLE, FeatureStatus.DOWNLOADING -> {
                        var completed = false
                        var failureMessage: String? = null
                        var lastBytes = 0L

                        model.download().collect { dl ->
                            when (dl) {
                                is DownloadStatus.DownloadProgress -> {
                                    lastBytes = dl.totalBytesDownloaded
                                }
                                is DownloadStatus.DownloadCompleted -> {
                                    completed = true
                                }
                                is DownloadStatus.DownloadFailed -> {
                                    failureMessage = dl.e.message ?: "AICore download failed"
                                }
                                else -> {}
                            }
                        }

                        if (completed) {
                            val res = JSObject()
                            res.put("ready", true)
                            res.put("status", "available")
                            res.put("detail", "Gemini Nano downloaded via Android AICore and is ready on-device.")
                            invoke.resolve(res)
                        } else if (failureMessage != null) {
                            invoke.reject("Gemini Nano download failed: $failureMessage")
                        } else {
                            val finalStatus = statusString(model.checkStatus())
                            val res = JSObject()
                            res.put("ready", finalStatus.first == "available")
                            res.put("status", finalStatus.first)
                            res.put(
                                "detail",
                                if (lastBytes > 0) {
                                    "${finalStatus.second} (${lastBytes / (1024 * 1024)} MB fetched)"
                                } else {
                                    finalStatus.second
                                },
                            )
                            invoke.resolve(res)
                        }
                    }
                    else -> {
                        val (label, detail) = statusString(current)
                        val res = JSObject()
                        res.put("ready", false)
                        res.put("status", label)
                        res.put("detail", detail)
                        invoke.resolve(res)
                    }
                }
            } catch (err: Throwable) {
                invoke.reject("Failed to prepare on-device model: ${err.message ?: err.javaClass.simpleName}")
            }
        }
    }

    @Command
    fun generate(invoke: Invoke) {
        val args = invoke.getArgs()
        val rawPrompt = args.getString("prompt")?.trim().orEmpty()
        if (rawPrompt.isEmpty()) {
            invoke.reject("Prompt cannot be empty.")
            return
        }
        val boundedPrompt =
            if (rawPrompt.length > MAX_PROMPT_CHARS) {
                rawPrompt.substring(0, MAX_PROMPT_CHARS)
            } else {
                rawPrompt
            }
        val requestedTokens = args.getInteger("max_tokens", DEFAULT_OUTPUT_TOKENS)
        val clampedTokens = requestedTokens.coerceIn(MIN_OUTPUT_TOKENS, MAX_OUTPUT_TOKENS)

        scope.launch {
            try {
                val model = getOrCreateModel()
                val statusInt = model.checkStatus()
                if (statusInt != FeatureStatus.AVAILABLE) {
                    val (label, _) = statusString(statusInt)
                    invoke.reject("aicore_unavailable:$label")
                    return@launch
                }

                val startedAt = System.currentTimeMillis()
                val request =
                    GenerateContentRequest.Builder(TextPart(boundedPrompt))
                        .apply {
                            temperature = 0.2f
                            candidateCount = 1
                            maxOutputTokens = clampedTokens
                        }
                        .build()

                val response = model.generateContent(request)
                val elapsedMs = (System.currentTimeMillis() - startedAt).coerceAtLeast(1L)
                val generatedText = response.candidates.firstOrNull()?.text?.trim().orEmpty()
                if (generatedText.isEmpty()) {
                    invoke.reject("On-device Gemini Nano returned an empty response.")
                    return@launch
                }

                val res = JSObject()
                res.put("text", generatedText)
                res.put("backend", BACKEND_ID)
                res.put("model_name", MODEL_DISPLAY_NAME)
                res.put("latency_ms", elapsedMs)
                invoke.resolve(res)
            } catch (err: Throwable) {
                invoke.reject("On-device generation failed: ${err.message ?: err.javaClass.simpleName}")
            }
        }
    }
}
