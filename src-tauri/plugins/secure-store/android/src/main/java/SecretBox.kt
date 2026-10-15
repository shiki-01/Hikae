package com.shiki01.hikae.securestore

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

// 小さな秘密情報（アクセストークン）を、Android Keystore の鍵（AES-256-GCM）で暗号化して保管する。
// 鍵は端末の外に出ない。値は SharedPreferences に「IV + 暗号文」の base64 で置く。
// Tauri に依存しない（端末上のテストから直接使えるように、プラグインの外に分けてある）
class SecretBox(
    private val context: Context,
    private val keyAlias: String = "hikae_secure_store_v1",
    private val prefsName: String = "hikae_secure_store"
) {
    private val ivSize = 12
    private val tagBits = 128

    private fun prefs() = context.getSharedPreferences(prefsName, Context.MODE_PRIVATE)

    private fun secretKey(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore")
        keyStore.load(null)
        (keyStore.getKey(keyAlias, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(
                keyAlias,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return generator.generateKey()
    }

    private fun encrypt(plain: String): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, secretKey())
        val encrypted = cipher.doFinal(plain.toByteArray(Charsets.UTF_8))
        return Base64.encodeToString(cipher.iv + encrypted, Base64.NO_WRAP)
    }

    private fun decrypt(stored: String): String {
        val bytes = Base64.decode(stored, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(
            Cipher.DECRYPT_MODE,
            secretKey(),
            GCMParameterSpec(tagBits, bytes, 0, ivSize)
        )
        return String(cipher.doFinal(bytes, ivSize, bytes.size - ivSize), Charsets.UTF_8)
    }

    fun put(key: String, value: String) {
        prefs().edit().putString(key, encrypt(value)).apply()
    }

    /** 無い、または復号できない（鍵が失われた、壊れている）ときは null。読めない値は捨てる */
    fun get(key: String): String? {
        val stored = prefs().getString(key, null) ?: return null
        return try {
            decrypt(stored)
        } catch (ex: Exception) {
            prefs().edit().remove(key).apply()
            null
        }
    }

    fun remove(key: String) {
        prefs().edit().remove(key).apply()
    }
}
