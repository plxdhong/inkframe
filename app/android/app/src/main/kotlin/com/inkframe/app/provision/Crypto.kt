package com.inkframe.app.provision

import org.bouncycastle.crypto.agreement.X25519Agreement
import org.bouncycastle.crypto.digests.SHA256Digest
import org.bouncycastle.crypto.generators.HKDFBytesGenerator
import org.bouncycastle.crypto.params.HKDFParameters
import org.bouncycastle.crypto.params.X25519PrivateKeyParameters
import org.bouncycastle.crypto.params.X25519PublicKeyParameters
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

/** Session crypto; must match crates/daemon/src/provision/crypto.rs byte for byte. */
object Crypto {
    private val KEY_INFO = "inkframe/prov/v1".toByteArray()
    val LABEL_PHONE = "phone".toByteArray()
    val LABEL_DEVICE = "device".toByteArray()

    class KeyPair(val secret: ByteArray) {
        private val priv = X25519PrivateKeyParameters(secret, 0)
        val public: ByteArray = priv.generatePublicKey().encoded

        fun agree(peer: ByteArray): ByteArray {
            require(peer.size == 32) { "peer key must be 32 bytes" }
            val out = ByteArray(32)
            X25519Agreement().apply { init(priv) }.calculateAgreement(X25519PublicKeyParameters(peer, 0), out, 0)
            if (out.all { it == 0.toByte() }) throw SecurityException("device sent a low-order key")
            return out
        }

        companion object {
            fun random(): KeyPair = KeyPair(ByteArray(32).also { SecureRandom().nextBytes(it) })
        }
    }

    class Keys(val confirm: ByteArray, val phoneToDevice: ByteArray, val deviceToPhone: ByteArray)

    fun derive(shared: ByteArray, code: String, salt: ByteArray, phonePub: ByteArray, devicePub: ByteArray): Keys {
        val ikm = shared + MessageDigest.getInstance("SHA-256").digest(code.toByteArray())
        val okm = ByteArray(96)
        HKDFBytesGenerator(SHA256Digest()).apply {
            init(HKDFParameters(ikm, salt, KEY_INFO + phonePub + devicePub))
            generateBytes(okm, 0, okm.size)
        }
        return Keys(okm.copyOfRange(0, 32), okm.copyOfRange(32, 64), okm.copyOfRange(64, 96))
    }

    fun proof(confirm: ByteArray, label: ByteArray): ByteArray =
        Mac.getInstance("HmacSHA256").apply { init(SecretKeySpec(confirm, "HmacSHA256")) }.doFinal(label)

    fun verifyProof(confirm: ByteArray, label: ByteArray, got: ByteArray): Boolean =
        MessageDigest.isEqual(proof(confirm, label), got)

    /** One direction of the sealed channel: AES-256-GCM, nonce = 4 zero bytes ‖ u64 BE counter. */
    class Channel(private val key: ByteArray) {
        private var counter = 0L

        private fun nonce(): ByteArray {
            val n = ByteArray(12)
            var c = counter++
            for (i in 11 downTo 4) {
                n[i] = (c and 0xff).toByte()
                c = c ushr 8
            }
            return n
        }

        private fun run(mode: Int, input: ByteArray): ByteArray =
            Cipher.getInstance("AES/GCM/NoPadding").apply {
                init(mode, SecretKeySpec(key, "AES"), GCMParameterSpec(128, nonce()))
            }.doFinal(input)

        fun seal(plain: ByteArray): ByteArray = run(Cipher.ENCRYPT_MODE, plain)

        /** Throws [javax.crypto.AEADBadTagException] on tampering, replay or loss. */
        fun open(sealed: ByteArray): ByteArray = run(Cipher.DECRYPT_MODE, sealed)
    }
}
