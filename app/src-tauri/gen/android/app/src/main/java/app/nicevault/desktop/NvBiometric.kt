package app.nicevault.desktop

import android.app.Activity
import android.content.pm.PackageManager
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

class BioError(val code: String, message: String) : Exception(message)

object NvBiometric {
  private const val STORE = "AndroidKeyStore"
  private const val GCM = "AES/GCM/NoPadding"

  fun status(activity: Activity): Map<String, Any> {
    val pm = activity.packageManager
    val kind = when {
      pm.hasSystemFeature(PackageManager.FEATURE_FINGERPRINT) -> "fingerprint"
      Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && pm.hasSystemFeature(PackageManager.FEATURE_FACE) -> "face"
      else -> "biometric"
    }
    val reason = if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
      "unsupported_os"
    } else {
      val bm = activity.getSystemService(BiometricManager::class.java)
      when (bm?.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG)) {
        BiometricManager.BIOMETRIC_SUCCESS -> ""
        BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> "none_enrolled"
        BiometricManager.BIOMETRIC_ERROR_NO_HARDWARE, BiometricManager.BIOMETRIC_ERROR_HW_UNAVAILABLE -> "no_hardware"
        else -> "unavailable"
      }
    }
    return mapOf("available" to reason.isEmpty(), "kind" to kind, "reason" to reason, "device" to (Build.MODEL ?: ""))
  }

  private fun keyStore(): KeyStore = KeyStore.getInstance(STORE).apply { load(null) }

  fun delete(alias: String) {
    try {
      keyStore().deleteEntry(alias)
    } catch (_: Exception) {
    }
  }

  private fun newKey(activity: Activity, alias: String): SecretKey {
    fun make(strongBox: Boolean): SecretKey {
      val b = KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .setUserAuthenticationRequired(true)
        .setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
        .setInvalidatedByBiometricEnrollment(true)
      if (strongBox) b.setIsStrongBoxBacked(true)
      val g = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, STORE)
      g.init(b.build())
      return g.generateKey()
    }
    delete(alias)
    if (activity.packageManager.hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE)) {
      try {
        return make(true)
      } catch (_: StrongBoxUnavailableException) {
      }
    }
    return make(false)
  }

  fun wrap(activity: Activity, alias: String, secret: ByteArray, title: String, cancel: String, done: (Result<ByteArray>) -> Unit) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return done(Result.failure(BioError("unsupported_os", "Android 11 or later is needed.")))
    val cipher = try {
      Cipher.getInstance(GCM).apply { init(Cipher.ENCRYPT_MODE, newKey(activity, alias)) }
    } catch (e: Exception) {
      secret.fill(0)
      return done(Result.failure(BioError("bio_failed", e.message ?: "keystore failed")))
    }
    prompt(activity, cipher, title, cancel) { r ->
      done(r.mapCatching { c ->
        try {
          c.iv + c.doFinal(secret)
        } finally {
          secret.fill(0)
        }
      })
      if (r.isFailure) {
        secret.fill(0)
        delete(alias)
      }
    }
  }

  fun unwrap(activity: Activity, alias: String, blob: ByteArray, title: String, cancel: String, done: (Result<ByteArray>) -> Unit) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return done(Result.failure(BioError("unsupported_os", "Android 11 or later is needed.")))
    val cipher = try {
      val key = keyStore().getKey(alias, null) as? SecretKey ?: throw BioError("bio_invalidated", "the key is gone")
      if (blob.size < 12 + 16) throw BioError("bio_invalidated", "bad blob")
      Cipher.getInstance(GCM).apply { init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, blob, 0, 12)) }
    } catch (e: KeyPermanentlyInvalidatedException) {
      delete(alias)
      return done(Result.failure(BioError("bio_invalidated", "fingerprints changed")))
    } catch (e: BioError) {
      return done(Result.failure(e))
    } catch (e: Exception) {
      return done(Result.failure(BioError("bio_failed", e.message ?: "keystore failed")))
    }
    prompt(activity, cipher, title, cancel) { r -> done(r.mapCatching { c -> c.doFinal(blob, 12, blob.size - 12) }) }
  }

  private fun prompt(activity: Activity, cipher: Cipher, title: String, cancel: String, done: (Result<Cipher>) -> Unit) {
    activity.runOnUiThread {
      var answered = false
      fun answer(r: Result<Cipher>) {
        if (!answered) {
          answered = true
          done(r)
        }
      }
      try {
        val p = BiometricPrompt.Builder(activity)
          .setTitle(title)
          .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
          .setNegativeButton(cancel, activity.mainExecutor) { _, _ -> answer(Result.failure(BioError("cancelled", "cancelled"))) }
          .build()
        p.authenticate(BiometricPrompt.CryptoObject(cipher), CancellationSignal(), activity.mainExecutor, object : BiometricPrompt.AuthenticationCallback() {
          override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
            val c = result.cryptoObject?.cipher
            answer(if (c != null) Result.success(c) else Result.failure(BioError("bio_failed", "no cipher")))
          }

          override fun onAuthenticationError(code: Int, msg: CharSequence) {
            val c = when (code) {
              BiometricPrompt.BIOMETRIC_ERROR_USER_CANCELED, BiometricPrompt.BIOMETRIC_ERROR_CANCELED -> "cancelled"
              BiometricPrompt.BIOMETRIC_ERROR_LOCKOUT, BiometricPrompt.BIOMETRIC_ERROR_LOCKOUT_PERMANENT -> "bio_lockout"
              else -> "bio_failed"
            }
            answer(Result.failure(BioError(c, msg.toString())))
          }
        })
      } catch (e: Exception) {
        answer(Result.failure(BioError("bio_failed", e.message ?: "prompt failed")))
      }
    }
  }
}
