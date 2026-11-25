package moe.dabney.personal

import android.app.Activity
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.nio.ByteBuffer
import java.nio.charset.StandardCharsets
import java.security.KeyStore
import java.time.Instant
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/**
 * Hardware-backed encrypted on-device personal vault for `moe.dabney.personal`.
 *
 * Uses Android Keystore (`AndroidKeyStore`) AES-256-GCM with a random 12-byte IV per
 * atomic write inside app-private `filesDir` (`android:allowBackup="false"` enforced by
 * `scripts/android-personal.sh`). Never bundled in the zero-PHI `moe.dabney.demo` app.
 *
 * Never writes personal records or decrypted payloads to logcat.
 */
@TauriPlugin
class PersonalVaultPlugin(private val activity: Activity) : Plugin(activity) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val vaultLock = Any()

    companion object {
        private const val KEYSTORE_PROVIDER = "AndroidKeyStore"
        private const val KEY_ALIAS = "dabney_personal_vault_v1"
        private const val CIPHER_TRANSFORMATION = "AES/GCM/NoPadding"
        private const val GCM_TAG_BITS = 128
        private const val VAULT_DIR = "dabney_personal_vault"
        private const val SNAPSHOT_FILE = "snapshot.enc"
        private const val MAGIC_HEADER = "DV01"
        private const val MAX_PLAINTEXT_BYTES = 16 * 1024 * 1024 // 16 MiB bounded cap
    }

    private fun vaultDir(): File {
        val dir = File(activity.filesDir, VAULT_DIR)
        if (!dir.exists()) {
            dir.mkdirs()
        }
        return dir
    }

    private fun snapshotFile(): File = File(vaultDir(), SNAPSHOT_FILE)

    private fun getOrCreateKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
        val existing = keyStore.getKey(KEY_ALIAS, null) as? SecretKey
        if (existing != null) {
            return existing
        }
        val keyGenerator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        val spec =
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .build()
        keyGenerator.init(spec)
        return keyGenerator.generateKey()
    }

    private fun encryptAndWriteAtomic(plaintextJson: String): Long {
        val plainBytes = plaintextJson.toByteArray(StandardCharsets.UTF_8)
        if (plainBytes.size > MAX_PLAINTEXT_BYTES) {
            throw IllegalArgumentException(
                "Vault payload (${plainBytes.size} bytes) exceeds 16 MiB on-device cap.",
            )
        }
        val key = getOrCreateKey()
        val cipher = Cipher.getInstance(CIPHER_TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val iv = cipher.iv
        val ciphertext = cipher.doFinal(plainBytes)

        val magic = MAGIC_HEADER.toByteArray(StandardCharsets.US_ASCII)
        val buf = ByteBuffer.allocate(magic.size + 1 + iv.size + ciphertext.size)
        buf.put(magic)
        buf.put(iv.size.toByte())
        buf.put(iv)
        buf.put(ciphertext)

        val dir = vaultDir()
        val tmpFile = File(dir, "$SNAPSHOT_FILE.tmp")
        val target = snapshotFile()
        tmpFile.writeBytes(buf.array())
        if (!tmpFile.renameTo(target)) {
            target.writeBytes(buf.array())
            tmpFile.delete()
        }
        return target.length()
    }

    private fun readAndDecryptLocked(): JSONObject? {
        val file = snapshotFile()
        if (!file.exists() || file.length() < 8) {
            return null
        }
        val raw = file.readBytes()
        if (raw.size < 6) {
            return null
        }
        val header = String(raw, 0, 4, StandardCharsets.US_ASCII)
        if (header != MAGIC_HEADER) {
            throw IllegalStateException("Unrecognized personal vault header.")
        }
        val ivLen = raw[4].toInt() and 0xFF
        if (ivLen <= 0 || raw.size <= 5 + ivLen) {
            throw IllegalStateException("Malformed personal vault IV length.")
        }
        val iv = raw.copyOfRange(5, 5 + ivLen)
        val ciphertext = raw.copyOfRange(5 + ivLen, raw.size)

        val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
        val key =
            keyStore.getKey(KEY_ALIAS, null) as? SecretKey
                ?: throw IllegalStateException("Personal vault encryption key missing from Android Keystore.")

        val cipher = Cipher.getInstance(CIPHER_TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_BITS, iv))
        val plainBytes = cipher.doFinal(ciphertext)
        return JSONObject(String(plainBytes, StandardCharsets.UTF_8))
    }

    private fun jsonArrayCount(raw: String?): Int {
        if (raw.isNullOrBlank()) return 0
        return try {
            JSONArray(raw).length()
        } catch (_: Exception) {
            0
        }
    }

    private fun buildStatusJsonLocked(doc: JSONObject?): JSObject {
        val file = snapshotFile()
        val totalBytes = if (file.exists()) file.length() else 0L
        val hasCgm = !doc?.optString("cgm_bridge_json", "").isNullOrBlank()
        val cgmCount = doc?.optInt("cgm_reading_count", 0) ?: 0
        val cgmSavedAt = doc?.optString("cgm_saved_at", "")?.takeIf { it.isNotBlank() }
        val checkinCount = jsonArrayCount(doc?.optString("checkins_json", ""))
        val noteCount = jsonArrayCount(doc?.optString("notes_json", ""))
        val chatTurnCount = jsonArrayCount(doc?.optString("chat_history_json", ""))

        val res = JSObject()
        res.put("enabled", true)
        res.put("encrypted", true)
        res.put("cipher", "AES-256-GCM (Android Keystore)")
        res.put("storage_backend", "android_keystore_files")
        res.put("has_cgm_snapshot", hasCgm)
        res.put("cgm_reading_count", cgmCount)
        if (cgmSavedAt != null) {
            res.put("cgm_saved_at", cgmSavedAt)
        } else {
            res.put("cgm_saved_at", JSONObject.NULL)
        }
        res.put("checkin_count", checkinCount)
        res.put("note_count", noteCount)
        res.put("chat_turn_count", chatTurnCount)
        res.put("total_bytes", totalBytes)
        return res
    }

    @Command
    fun getStatus(invoke: Invoke) {
        scope.launch {
            try {
                val res =
                    synchronized(vaultLock) {
                        val doc =
                            try {
                                readAndDecryptLocked()
                            } catch (_: Exception) {
                                null
                            }
                        buildStatusJsonLocked(doc)
                    }
                invoke.resolve(res)
            } catch (err: Exception) {
                invoke.reject("Failed to query personal vault status: ${err.message ?: err.javaClass.simpleName}")
            }
        }
    }

    @Command
    fun loadSnapshot(invoke: Invoke) {
        scope.launch {
            try {
                val res =
                    synchronized(vaultLock) {
                        val doc = readAndDecryptLocked()
                        val out = JSObject()
                        if (doc == null) {
                            out.put("found", false)
                            out.put("cgm_bridge_json", JSONObject.NULL)
                            out.put("cgm_source_label", JSONObject.NULL)
                            out.put("cgm_saved_at", JSONObject.NULL)
                            out.put("cgm_reading_count", 0)
                            out.put("checkins_json", JSONObject.NULL)
                            out.put("notes_json", JSONObject.NULL)
                            out.put("chat_history_json", JSONObject.NULL)
                        } else {
                            out.put("found", true)
                            putOptString(out, "cgm_bridge_json", doc.optString("cgm_bridge_json", ""))
                            putOptString(out, "cgm_source_label", doc.optString("cgm_source_label", ""))
                            putOptString(out, "cgm_saved_at", doc.optString("cgm_saved_at", ""))
                            out.put("cgm_reading_count", doc.optInt("cgm_reading_count", 0))
                            putOptString(out, "checkins_json", doc.optString("checkins_json", ""))
                            putOptString(out, "notes_json", doc.optString("notes_json", ""))
                            putOptString(out, "chat_history_json", doc.optString("chat_history_json", ""))
                        }
                        out
                    }
                invoke.resolve(res)
            } catch (err: Exception) {
                invoke.reject("Failed to decrypt personal vault snapshot: ${err.message ?: err.javaClass.simpleName}")
            }
        }
    }

    private fun putOptString(obj: JSObject, key: String, value: String) {
        if (value.isNotBlank()) {
            obj.put(key, value)
        } else {
            obj.put(key, JSONObject.NULL)
        }
    }

    @Command
    fun saveSnapshot(invoke: Invoke) {
        val args = invoke.getArgs()
        scope.launch {
            try {
                val res =
                    synchronized(vaultLock) {
                        val merged =
                            try {
                                readAndDecryptLocked() ?: JSONObject()
                            } catch (_: Exception) {
                                JSONObject()
                            }

                        if (args.has("cgm_bridge_json") && !args.isNull("cgm_bridge_json")) {
                            val cgm = args.getString("cgm_bridge_json")
                            if (cgm.isNullOrBlank()) {
                                merged.remove("cgm_bridge_json")
                                merged.remove("cgm_source_label")
                                merged.remove("cgm_saved_at")
                                merged.put("cgm_reading_count", 0)
                            } else {
                                merged.put("cgm_bridge_json", cgm)
                                merged.put("cgm_saved_at", Instant.now().toString())
                            }
                        }
                        if (args.has("cgm_source_label") && !args.isNull("cgm_source_label")) {
                            merged.put("cgm_source_label", args.getString("cgm_source_label"))
                        }
                        if (args.has("cgm_reading_count") && !args.isNull("cgm_reading_count")) {
                            merged.put("cgm_reading_count", args.getInteger("cgm_reading_count", 0))
                        }
                        if (args.has("checkins_json") && !args.isNull("checkins_json")) {
                            merged.put("checkins_json", args.getString("checkins_json"))
                        }
                        if (args.has("notes_json") && !args.isNull("notes_json")) {
                            merged.put("notes_json", args.getString("notes_json"))
                        }
                        if (args.has("chat_history_json") && !args.isNull("chat_history_json")) {
                            merged.put("chat_history_json", args.getString("chat_history_json"))
                        }

                        merged.put("updated_at", Instant.now().toString())
                        encryptAndWriteAtomic(merged.toString())
                        buildStatusJsonLocked(merged)
                    }
                invoke.resolve(res)
            } catch (err: Exception) {
                invoke.reject("Failed to encrypt personal vault snapshot: ${err.message ?: err.javaClass.simpleName}")
            }
        }
    }

    @Command
    fun wipeVault(invoke: Invoke) {
        scope.launch {
            try {
                val res =
                    synchronized(vaultLock) {
                        val dir = vaultDir()
                        dir.listFiles()?.forEach { file ->
                            try {
                                if (file.isFile && file.length() in 1..MAX_PLAINTEXT_BYTES) {
                                    file.writeBytes(ByteArray(file.length().toInt()))
                                }
                            } catch (_: Exception) {
                                // Best-effort overwrite before delete.
                            }
                            file.delete()
                        }
                        try {
                            val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
                            if (keyStore.containsAlias(KEY_ALIAS)) {
                                keyStore.deleteEntry(KEY_ALIAS)
                            }
                        } catch (_: Exception) {
                            // Key may not exist if vault was never written.
                        }
                        buildStatusJsonLocked(null)
                    }
                invoke.resolve(res)
            } catch (err: Exception) {
                invoke.reject("Failed to wipe personal vault: ${err.message ?: err.javaClass.simpleName}")
            }
        }
    }
}
