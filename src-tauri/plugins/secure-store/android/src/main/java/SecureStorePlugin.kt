package com.shiki01.hikae.securestore

import android.app.Activity
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class KeyArgs {
    lateinit var key: String
}

@InvokeArg
class SetArgs {
    lateinit var key: String
    lateinit var value: String
}

// 小さな秘密情報（アクセストークン）を、Android Keystore の鍵（AES-256-GCM）で暗号化して保管する。
// 鍵は端末の外に出ない。値は SharedPreferences に「IV + 暗号文」の base64 で置く。
@TauriPlugin
class SecureStorePlugin(private val activity: Activity) : Plugin(activity) {
    private val keyAlias = "hikae_secure_store_v1"
    private val prefsName = "hikae_secure_store"
    private val ivSize = 12
    private val tagBits = 128

    private fun prefs() = activity.getSharedPreferences(prefsName, Context.MODE_PRIVATE)

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

    @Command
    fun set(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SetArgs::class.java)
            prefs().edit().putString(args.key, encrypt(args.value)).apply()
            invoke.resolve()
        } catch (ex: Exception) {
            invoke.reject("set failed")
        }
    }

    @Command
    fun get(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(KeyArgs::class.java)
            val stored = prefs().getString(args.key, null)
            val result = JSObject()
            if (stored != null) {
                try {
                    result.put("value", decrypt(stored))
                } catch (ex: Exception) {
                    // 鍵が失われた（バックアップの復元など）ときは、読めない値を捨てて「無い」として返す
                    prefs().edit().remove(args.key).apply()
                }
            }
            invoke.resolve(result)
        } catch (ex: Exception) {
            invoke.reject("get failed")
        }
    }

    @Command
    fun delete(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(KeyArgs::class.java)
            prefs().edit().remove(args.key).apply()
            invoke.resolve()
        } catch (ex: Exception) {
            invoke.reject("delete failed")
        }
    }
}
