package com.inkframe.app.provision

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import java.util.Base64

class WrongCodeException : ProtocolException("配网码不正确")

class DeviceErrorException(val code: String, message: String) : ProtocolException(message)

/**
 * Phone side of docs/PROVISIONING.md: handshake with the setup code, then
 * sealed requests. One instance per connection; calls must not overlap.
 */
class ProvisioningClient(
    private val transport: Transport,
    private val newKeyPair: () -> Crypto.KeyPair = { Crypto.KeyPair.random() },
) {
    private val reassembler = Frames.Reassembler()
    private var tx: Crypto.Channel? = null
    private var rx: Crypto.Channel? = null
    private val b64d = Base64.getDecoder()

    val authenticated get() = tx != null

    suspend fun info(): DeviceInfo = Messages.json.decodeFromString(transport.readInfo().decodeToString())

    private class Handshake(val kp: Crypto.KeyPair, val devicePub: ByteArray, val salt: ByteArray)
    private var handshake: Handshake? = null

    /**
     * Starts the handshake. The frame paints its setup code when it sees this,
     * so call it right after connecting: the user can't type a code before it
     * is on the panel.
     */
    suspend fun hello() {
        tx = null
        rx = null
        handshake = null
        val kp = newKeyPair()
        send(Messages.hello(kp.public))
        val hello = expect(receiveOuter(), "hello")
        handshake = Handshake(kp, b64d.decode(hello.str("pub")), b64d.decode(hello.str("salt")))
    }

    /** Proves knowledge of [code] and checks the device's proof. Throws [WrongCodeException]. */
    suspend fun authenticate(code: String) {
        val normalized = SetupCode.normalize(code) ?: throw WrongCodeException()
        // A failed attempt sends the device back to awaiting hello, so each try needs a fresh one.
        val h = handshake ?: run { hello(); handshake } ?: throw ProtocolException("no handshake")
        handshake = null
        val kp = h.kp
        val devicePub = h.devicePub
        val salt = h.salt
        val keys = Crypto.derive(kp.agree(devicePub), normalized, salt, kp.public, devicePub)
        send(Messages.auth(Crypto.proof(keys.confirm, Crypto.LABEL_PHONE)))
        val reply = receiveOuter()
        if (reply.str("t") == "error" && reply.str("code") == "bad_code") throw WrongCodeException()
        val mac = b64d.decode(expect(reply, "auth").str("mac"))
        if (!Crypto.verifyProof(keys.confirm, Crypto.LABEL_DEVICE, mac)) {
            throw SecurityException("设备未能证明配网码，可能不是你的相框")
        }
        tx = Crypto.Channel(keys.phoneToDevice)
        rx = Crypto.Channel(keys.deviceToPhone)
    }

    suspend fun scan(): List<Network> {
        sendSealed(Messages.scan)
        val r = expect(receiveSealed(SCAN_TIMEOUT_MS), "networks")
        val list = r["list"] ?: throw ProtocolException("networks without list")
        return Messages.json.decodeFromJsonElement(ListSerializer(Network.serializer()), list)
    }

    /** Emits `Connecting`, then the final `Connected` or `Failed`. */
    fun connect(ssid: String, psk: String, hidden: Boolean): Flow<WifiState> = flow {
        sendSealed(Messages.connect(ssid, psk, hidden))
        while (true) {
            val state = parseState(expect(receiveSealed(CONNECT_TIMEOUT_MS), "state"))
            emit(state)
            if (state !is WifiState.Connecting) break
        }
    }

    suspend fun status(): WifiState {
        sendSealed(Messages.status)
        return parseState(expect(receiveSealed(REPLY_TIMEOUT_MS), "state"))
    }

    /** Tells the device to leave provisioning mode (it repaints the schedule). */
    suspend fun done() {
        sendSealed(Messages.done)
        runCatching { expect(receiveSealed(REPLY_TIMEOUT_MS), "bye") }
    }

    private fun parseState(o: JsonObject): WifiState = Messages.stateJson.decodeFromJsonElement(WifiState.serializer(), o)

    private suspend fun send(text: String) {
        for (f in Frames.fragment(text.toByteArray(), transport.maxFragment)) transport.write(f)
    }

    private suspend fun sendSealed(inner: String) {
        val ch = tx ?: throw ProtocolException("not authenticated")
        send(Messages.sealed(ch.seal(inner.toByteArray())))
    }

    private suspend fun receiveOuter(timeoutMs: Long = REPLY_TIMEOUT_MS): JsonObject = withTimeout(timeoutMs) {
        var msg: ByteArray? = null
        while (msg == null) msg = reassembler.push(transport.receive())
        Messages.parse(msg.decodeToString())
    }

    private suspend fun receiveSealed(timeoutMs: Long): JsonObject {
        val outer = receiveOuter(timeoutMs)
        if (outer.str("t") == "error") throw DeviceErrorException(outer.str("code"), outer.str("msg"))
        val ch = rx ?: throw ProtocolException("not authenticated")
        val inner = Messages.parse(ch.open(b64d.decode(expect(outer, "sealed").str("c"))).decodeToString())
        if (inner.str("t") == "error") throw DeviceErrorException(inner.str("code"), inner.str("msg"))
        return inner
    }

    private fun expect(o: JsonObject, t: String): JsonObject {
        val got = o.str("t")
        if (got == "error") throw DeviceErrorException(o.str("code"), o.str("msg"))
        if (got != t) throw ProtocolException("expected '$t', got '$got'")
        return o
    }

    private fun JsonObject.str(key: String): String = this[key]?.jsonPrimitive?.contentOrNull ?: ""

    companion object {
        const val REPLY_TIMEOUT_MS = 15_000L
        const val SCAN_TIMEOUT_MS = 40_000L
        const val CONNECT_TIMEOUT_MS = 90_000L
    }
}
