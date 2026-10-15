package com.shiki01.hikae.securestore

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

// 端末（エミュレータ）上で、Android Keystore を使う SecretBox の動きを確かめる
@RunWith(AndroidJUnit4::class)
class SecretBoxTest {
    private val context: Context = ApplicationProvider.getApplicationContext()
    private val prefsName = "hikae_secret_box_test"
    private val box = SecretBox(context, "hikae_secret_box_test_key", prefsName)

    @Before
    fun clean() {
        context.getSharedPreferences(prefsName, Context.MODE_PRIVATE).edit().clear().commit()
    }

    @After
    fun tearDown() {
        context.getSharedPreferences(prefsName, Context.MODE_PRIVATE).edit().clear().commit()
    }

    @Test
    fun roundTripKeepsTheValue() {
        val token = "gho_テスト-0123456789abcdef"
        box.put("github-token", token)
        assertEquals(token, box.get("github-token"))
    }

    @Test
    fun theStoredValueIsNotPlaintext() {
        val token = "gho_plain_text_must_not_appear"
        box.put("github-token", token)
        val raw = context.getSharedPreferences(prefsName, Context.MODE_PRIVATE)
            .getString("github-token", null)
        assertNotNull(raw)
        assertFalse(raw!!.contains(token))
        assertFalse(raw.contains("gho_"))
    }

    @Test
    fun theSameValueIsEncryptedDifferentlyEachTime() {
        box.put("a", "same")
        val first = context.getSharedPreferences(prefsName, Context.MODE_PRIVATE).getString("a", null)
        box.put("a", "same")
        val second = context.getSharedPreferences(prefsName, Context.MODE_PRIVATE).getString("a", null)
        assertTrue(first != second)
    }

    @Test
    fun missingKeyIsNull() {
        assertNull(box.get("nothing"))
    }

    @Test
    fun removeDeletesTheValue() {
        box.put("github-token", "x")
        box.remove("github-token")
        assertNull(box.get("github-token"))
    }

    @Test
    fun aCorruptedValueIsDroppedAndReadsAsMissing() {
        box.put("github-token", "x")
        context.getSharedPreferences(prefsName, Context.MODE_PRIVATE).edit()
            .putString("github-token", "bm90LWEtdmFsaWQtY2lwaGVydGV4dA==").commit()
        assertNull(box.get("github-token"))
        // 読めない値は消えている
        assertNull(context.getSharedPreferences(prefsName, Context.MODE_PRIVATE).getString("github-token", null))
    }

    @Test
    fun aLongValueSurvives() {
        val value = "a".repeat(4000)
        box.put("long", value)
        assertEquals(value, box.get("long"))
    }
}
