package io.github.yustulinu.docket.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.docket_core.generateKey

/**
 * The vault's 256-bit key. The core generates it; an AES-GCM key that never leaves the Android
 * Keystore (hardware-backed where the phone has it) wraps it, and only the wrapped form is stored.
 * Copying the app's files to another phone gives nothing: the wrapping key stays on this one.
 */
object VaultKey {
    private const val ALIAS = "docket-vault-key"
    private const val FILE = "vault.key"

    fun load(context: Context): ByteArray {
        val file = File(context.filesDir, FILE)
        if (file.exists()) return unwrap(file.readBytes())
        val key = generateKey()
        val tmp = File(context.filesDir, "$FILE.tmp")
        tmp.writeBytes(wrap(key))
        tmp.renameTo(file)
        return key
    }

    private fun keystoreKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    /** iv (12 bytes) followed by the ciphertext and tag. */
    private fun wrap(key: ByteArray): ByteArray {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.ENCRYPT_MODE, keystoreKey())
        return c.iv + c.doFinal(key)
    }

    private fun unwrap(data: ByteArray): ByteArray {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.DECRYPT_MODE, keystoreKey(), GCMParameterSpec(128, data, 0, 12))
        return c.doFinal(data, 12, data.size - 12)
    }
}
