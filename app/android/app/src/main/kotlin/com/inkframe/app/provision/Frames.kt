package com.inkframe.app.provision

/** Fragmentation over GATT; see frame.rs. Header byte bit 0 = more fragments follow. */
object Frames {
    const val MORE: Int = 0x01
    const val MAX_MESSAGE = 4096
    const val MIN_CHUNK = 19

    fun fragment(msg: ByteArray, maxFragment: Int): List<ByteArray> {
        val chunk = maxOf(maxFragment - 1, MIN_CHUNK)
        if (msg.isEmpty()) return listOf(byteArrayOf(0))
        val pieces = (msg.indices step chunk).map { msg.copyOfRange(it, minOf(it + chunk, msg.size)) }
        return pieces.mapIndexed { i, p -> byteArrayOf(if (i < pieces.lastIndex) MORE.toByte() else 0) + p }
    }

    class Reassembler {
        private val buf = java.io.ByteArrayOutputStream()

        /** Returns the full message once its last fragment arrives. */
        fun push(fragment: ByteArray): ByteArray? {
            if (fragment.isEmpty()) {
                buf.reset()
                throw ProtocolException("empty fragment")
            }
            val header = fragment[0].toInt() and 0xff
            if (header and MORE.inv() != 0) {
                buf.reset()
                throw ProtocolException("reserved header bits set")
            }
            if (buf.size() + fragment.size - 1 > MAX_MESSAGE) {
                buf.reset()
                throw ProtocolException("message too long")
            }
            buf.write(fragment, 1, fragment.size - 1)
            if (header and MORE != 0) return null
            return buf.toByteArray().also { buf.reset() }
        }
    }
}

open class ProtocolException(message: String) : Exception(message)
