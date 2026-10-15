package com.shiki01.hikae.securestore

import android.app.Activity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class KeyArgs {
    lateinit var key: String
}

@InvokeArg
class SetArgs {
    lateinit var key: String
    lateinit var value: String
}

// Rust（tauri-plugin-hikae-secure-store）から呼ばれる。暗号化は SecretBox が行う。
// 失敗の理由（値や鍵に関わる情報）は返さない
@TauriPlugin
class SecureStorePlugin(activity: Activity) : Plugin(activity) {
    private val box = SecretBox(activity)

    @Command
    fun set(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(SetArgs::class.java)
            box.put(args.key, args.value)
            invoke.resolve()
        } catch (ex: Exception) {
            invoke.reject("set failed")
        }
    }

    @Command
    fun get(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(KeyArgs::class.java)
            val result = JSObject()
            box.get(args.key)?.let { result.put("value", it) }
            invoke.resolve(result)
        } catch (ex: Exception) {
            invoke.reject("get failed")
        }
    }

    @Command
    fun delete(invoke: Invoke) {
        try {
            val args = invoke.parseArgs(KeyArgs::class.java)
            box.remove(args.key)
            invoke.resolve()
        } catch (ex: Exception) {
            invoke.reject("delete failed")
        }
    }
}
