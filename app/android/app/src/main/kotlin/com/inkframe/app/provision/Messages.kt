package com.inkframe.app.provision

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.util.Base64

/** GATT layout; must match crates/daemon/src/provision/mod.rs. */
object Gatt {
    const val SERVICE = "6f1a0001-8b5e-4c1f-9d3a-1e5f0a7c2b90"
    const val INFO = "6f1a0002-8b5e-4c1f-9d3a-1e5f0a7c2b90"
    const val RX = "6f1a0003-8b5e-4c1f-9d3a-1e5f0a7c2b90"
    const val TX = "6f1a0004-8b5e-4c1f-9d3a-1e5f0a7c2b90"
    const val CCCD = "00002902-0000-1000-8000-00805f9b34fb"
    const val PROTO_VERSION = 1
}

@Serializable
data class DeviceInfo(val proto: Int, val id: String, val name: String, val fw: String = "")

@Serializable
data class Network(val ssid: String, val signal: Int, val secure: Boolean)

@Serializable
enum class FailReason {
    @SerialName("auth_failed") AUTH_FAILED,
    @SerialName("not_found") NOT_FOUND,
    @SerialName("timeout") TIMEOUT,
    @SerialName("error") ERROR,
}

/** The device's Wi-Fi as reported over BLE (`WifiState` in wifi.rs). */
@Serializable
sealed interface WifiState {
    @Serializable @SerialName("idle")
    data class Idle(val ssid: String? = null, val ip: String? = null) : WifiState

    @Serializable @SerialName("connecting")
    data class Connecting(val ssid: String) : WifiState

    @Serializable @SerialName("connected")
    data class Connected(val ssid: String, val ip: String, val host: String, val port: Int) : WifiState

    @Serializable @SerialName("failed")
    data class Failed(val ssid: String, val reason: FailReason, val detail: String = "") : WifiState
}

/** Wire encoding. Key order matters for the cross-language test vector. */
object Messages {
    private val b64 = Base64.getEncoder()
    val json = Json { ignoreUnknownKeys = true }
    val stateJson = Json { ignoreUnknownKeys = true; classDiscriminator = "state" }

    fun hello(pub: ByteArray) = buildJsonObject {
        put("t", "hello"); put("v", Gatt.PROTO_VERSION); put("pub", b64.encodeToString(pub))
    }.toString()

    fun auth(mac: ByteArray) = buildJsonObject { put("t", "auth"); put("mac", b64.encodeToString(mac)) }.toString()

    fun sealed(ct: ByteArray) = buildJsonObject { put("t", "sealed"); put("c", b64.encodeToString(ct)) }.toString()

    val scan = buildJsonObject { put("t", "scan") }.toString()
    val status = buildJsonObject { put("t", "status") }.toString()
    val done = buildJsonObject { put("t", "done") }.toString()

    fun connect(ssid: String, psk: String, hidden: Boolean) = buildJsonObject {
        put("t", "connect"); put("ssid", ssid); put("psk", psk); put("hidden", hidden)
    }.toString()

    fun parse(text: String): JsonObject = json.parseToJsonElement(text) as? JsonObject
        ?: throw ProtocolException("expected a JSON object")
}
