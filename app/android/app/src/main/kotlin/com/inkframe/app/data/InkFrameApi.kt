package com.inkframe.app.data

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.plugins.HttpTimeout
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.plugins.timeout
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.client.request.parameter
import io.ktor.client.request.patch
import io.ktor.client.request.post
import io.ktor.client.request.put
import io.ktor.client.request.setBody
import io.ktor.client.statement.HttpResponse
import io.ktor.client.statement.bodyAsText
import io.ktor.http.ContentType
import io.ktor.http.contentType
import io.ktor.http.isSuccess
import io.ktor.serialization.kotlinx.json.json
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

val AppJson = Json { ignoreUnknownKeys = true; explicitNulls = false }

fun newHttpClient() = HttpClient {
    expectSuccess = false
    install(ContentNegotiation) { json(AppJson) }
    install(HttpTimeout) {
        connectTimeoutMillis = 5_000
        requestTimeoutMillis = 30_000
        socketTimeoutMillis = 30_000
    }
}

/** HTTP API v1 of one frame (see crates/daemon/src/http.rs). */
class InkFrameApi(private val http: HttpClient, val device: Device) {
    private val api = "${device.baseUrl}/api/v1"

    fun previewUrl(bust: Any) = "$api/preview.png?t=$bust"
    fun renderUrl(view: String, bust: Any) = "$api/render/$view.png?t=$bust"
    fun photoUrl(id: String) = "$api/photos/$id/source.png"

    private suspend fun HttpResponse.ok(): HttpResponse {
        if (status.isSuccess()) return this
        val msg = runCatching { AppJson.parseToJsonElement(bodyAsText()).jsonObject["error"]?.jsonPrimitive?.content }.getOrNull()
        throw ApiException(msg ?: "HTTP ${status.value}")
    }

    private fun json(block: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit) = buildJsonObject(block)

    suspend fun status(): Status = http.get("$api/status").ok().body()

    suspend fun refresh(target: String) {
        http.post("$api/refresh") { contentType(ContentType.Application.Json); setBody(json { put("target", target) }) }.ok()
    }

    suspend fun setMode(mode: String) {
        http.put("$api/mode") { contentType(ContentType.Application.Json); setBody(json { put("mode", mode) }) }.ok()
    }

    /** [action] is "shutdown" or "reboot"; the frame answers before it goes down. */
    suspend fun power(action: String): PowerAccepted =
        http.post("$api/power") { contentType(ContentType.Application.Json); setBody(json { put("action", action) }) }.ok().body()

    /** The whole config as JSON, so fields this app version does not know survive a save. */
    suspend fun config(): JsonObject = http.get("$api/config").ok().body()

    suspend fun saveConfig(cfg: JsonObject): ConfigSaved =
        http.put("$api/config") { contentType(ContentType.Application.Json); setBody(cfg) }.ok().body()

    suspend fun photos(): List<PhotoMeta> = http.get("$api/photos").ok().body()

    /** Dithers [png] with the panel palette without storing it. */
    suspend fun previewPhoto(png: ByteArray): ByteArray = http.post("$api/photos/preview") {
        timeout { requestTimeoutMillis = 60_000 }
        contentType(ContentType.Image.PNG)
        setBody(png)
    }.ok().body()

    suspend fun uploadPhoto(png: ByteArray): PhotoMeta = http.post("$api/photos") {
        timeout { requestTimeoutMillis = 60_000 }
        contentType(ContentType.Image.PNG)
        setBody(png)
    }.ok().body()

    suspend fun updatePhoto(id: String, favorite: Boolean? = null, paused: Boolean? = null): PhotoMeta =
        http.patch("$api/photos/$id") {
            contentType(ContentType.Application.Json)
            setBody(json {
                favorite?.let { put("favorite", it) }
                paused?.let { put("paused", it) }
            })
        }.ok().body()

    suspend fun deletePhoto(id: String) {
        http.delete("$api/photos/$id").ok()
    }

    suspend fun showPhoto(id: String) {
        http.post("$api/photos/$id/show").ok()
    }

    /** Re-dithers every photo after enhancement or palette changes. */
    suspend fun reprocessPhotos() {
        http.post("$api/photos/reprocess") { timeout { requestTimeoutMillis = 300_000 } }.ok()
    }

    /** QWeather GeoAPI with the frame's own API host and key. */
    suspend fun searchCity(apiHost: String, apiKey: String, query: String): List<City> {
        val base = if (apiHost.startsWith("http")) apiHost else "https://$apiHost"
        val body: JsonObject = http.get("$base/geo/v2/city/lookup") {
            header("X-QW-Api-Key", apiKey)
            parameter("location", query)
            parameter("number", 10)
        }.ok().body()
        val code = body["code"]?.jsonPrimitive?.content
        if (code != "200") throw ApiException(if (code == "404") "没有找到「$query」" else "城市查询失败 ($code)")
        return body["location"]?.jsonArray.orEmpty().map {
            val o = it.jsonObject
            fun s(k: String) = o[k]?.jsonPrimitive?.content.orEmpty()
            City(s("id"), s("name"), listOf(s("adm1"), s("adm2")).filter(String::isNotEmpty).distinct().joinToString(" · "))
        }
    }
}
