package com.inkframe.app.provision

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** Plays the device from a script and records what the phone sends. */
private class ScriptedTransport(replies: List<String>, override val maxFragment: Int = 20) : Transport {
    val sent = mutableListOf<String>()
    private val reasm = Frames.Reassembler()
    private val queue = ArrayDeque(replies.flatMap { Frames.fragment(it.toByteArray(), maxFragment) })

    override suspend fun readInfo() = """{"proto":1,"id":"3fa2","name":"inkframe-3fa2","fw":"0.1.0"}""".toByteArray()

    override suspend fun write(fragment: ByteArray) {
        reasm.push(fragment)?.let { sent += it.decodeToString() }
    }

    override suspend fun receive(): ByteArray = queue.removeFirstOrNull() ?: throw TransportClosedException()

    override fun close() {}
}

class ProtocolTest {
    /** Written by `cross_language_vector` in session.rs. */
    private val vector: Map<String, String> =
        File("../../../crates/daemon/src/provision/testdata/vector.txt").readLines()
            .filter { '=' in it }
            .associate { it.substringBefore('=') to it.substringAfter('=') }

    private val phoneKey = { Crypto.KeyPair(ByteArray(32) { 1 }) }

    @Test
    fun matchesRustTranscript() = runBlocking {
        val t = ScriptedTransport(listOf(vector.getValue("device_hello"), vector.getValue("device_auth"), vector.getValue("sealed_bye")))
        val client = ProvisioningClient(t, phoneKey)
        // Typed loosely on purpose: normalisation must give the same code.
        client.authenticate(" 7k3m-9qxr-2d4f ")
        assertEquals(vector.getValue("phone_hello"), t.sent[0])
        assertEquals(vector.getValue("phone_auth"), t.sent[1])
        assertTrue(client.authenticated)
        // The device's first sealed message decrypts.
        client.done()
        assertEquals(3, t.sent.size)
    }

    @Test
    fun helloGoesOutBeforeTheCodeIsKnown() = runBlocking {
        // The frame only paints the code after hello, so hello must not wait for the code.
        val t = ScriptedTransport(listOf(vector.getValue("device_hello"), vector.getValue("device_auth")))
        val client = ProvisioningClient(t, phoneKey)
        client.hello()
        assertEquals(listOf(vector.getValue("phone_hello")), t.sent)
        client.authenticate("7K3M9QXR2D4F")
        assertEquals(vector.getValue("phone_auth"), t.sent[1])
        assertEquals(2, t.sent.size)
        assertTrue(client.authenticated)
    }

    @Test
    fun retryAfterWrongCodeSaysHelloAgain() = runBlocking {
        val badCode = """{"t":"error","code":"bad_code","msg":"x"}"""
        val t = ScriptedTransport(listOf(vector.getValue("device_hello"), badCode, vector.getValue("device_hello"), vector.getValue("device_auth")))
        val client = ProvisioningClient(t, phoneKey)
        client.hello()
        assertThrows(WrongCodeException::class.java) { runBlocking { client.authenticate("7K3M9QXR2D4G") } }
        client.authenticate("7K3M9QXR2D4F")
        assertEquals(vector.getValue("phone_hello"), t.sent[2])
        assertTrue(client.authenticated)
    }

    @Test
    fun sealedRequestMatchesRust() = runBlocking {
        val t = ScriptedTransport(listOf(vector.getValue("device_hello"), vector.getValue("device_auth")))
        val client = ProvisioningClient(t, phoneKey)
        client.authenticate("7K3M9QXR2D4F")
        // No reply scripted, so the scan fails after sending; only the bytes matter.
        runCatching { client.scan() }
        assertEquals(vector.getValue("sealed_scan"), t.sent[2])
    }

    @Test
    fun wrongCodeIsReported() {
        val t = ScriptedTransport(listOf(vector.getValue("device_hello"), """{"t":"error","code":"bad_code","msg":"x"}"""))
        assertThrows(WrongCodeException::class.java) {
            runBlocking { ProvisioningClient(t, phoneKey).authenticate("7K3M9QXR2D4G") }
        }
    }

    @Test
    fun forgedDeviceProofIsRejected() {
        val forged = """{"t":"auth","mac":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}"""
        val t = ScriptedTransport(listOf(vector.getValue("device_hello"), forged))
        assertThrows(SecurityException::class.java) {
            runBlocking { ProvisioningClient(t, phoneKey).authenticate("7K3M9QXR2D4F") }
        }
    }

    @Test
    fun setupCodeNormalisation() {
        assertEquals("7K3M9QXR2D4F", SetupCode.normalize("7k3m-9qxr-2d4f"))
        assertEquals("011100000000", SetupCode.normalize("o1il 0000 0000"))
        assertNull(SetupCode.normalize("7K3M-9QXR-2D4"))
        assertNull(SetupCode.normalize("7K3M-9QXR-2D4U"))
        assertEquals("7K3M-9QXR-2D4F", SetupCode.group("7K3M9QXR2D4F"))
    }

    @Test
    fun fragmentsRoundTrip() {
        for (len in listOf(0, 1, 18, 19, 20, 243, 244, 1000, Frames.MAX_MESSAGE)) {
            val msg = ByteArray(len) { it.toByte() }
            val r = Frames.Reassembler()
            var out: ByteArray? = null
            val frags = Frames.fragment(msg, 244)
            assertTrue(frags.all { it.size <= 244 })
            frags.forEach { out = r.push(it) }
            assertArrayEquals(msg, out)
        }
        assertThrows(ProtocolException::class.java) { Frames.Reassembler().push(byteArrayOf(0x80.toByte(), 1)) }
    }

    @Test
    fun wifiStatesDecode() {
        val c = Messages.stateJson.decodeFromString(
            WifiState.serializer(),
            """{"t":"state","state":"connected","ssid":"Home","ip":"10.0.2.2","host":"inkframe-3fa2.local","port":8080}""",
        )
        assertEquals(WifiState.Connected("Home", "10.0.2.2", "inkframe-3fa2.local", 8080), c)
        val f = Messages.stateJson.decodeFromString(
            WifiState.serializer(),
            """{"t":"state","state":"failed","ssid":"Home","reason":"auth_failed","detail":"x"}""",
        )
        assertEquals(FailReason.AUTH_FAILED, (f as WifiState.Failed).reason)
    }
}
