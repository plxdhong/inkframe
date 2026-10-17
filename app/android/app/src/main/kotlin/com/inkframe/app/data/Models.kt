package com.inkframe.app.data

import kotlinx.serialization.Serializable

/** A frame the app knows about. [host] is an IP or mDNS name. */
@Serializable
data class Device(val id: String, val name: String, val host: String, val port: Int = 8080) {
    val baseUrl get() = if (':' in host) "http://[$host]:$port" else "http://$host:$port"
}

/** The parts of `GET /api/v1/status` the app shows; unknown fields are ignored. */
@Serializable
data class Status(
    val version: String = "",
    val now: String = "",
    val workday: Boolean = true,
    val mode: String = "auto",
    val healthy: Boolean = true,
    val next_event: Event? = null,
    val shown: Shown? = null,
    val tasks: List<TaskStatus> = emptyList(),
    val weather: WeatherInfo? = null,
    val network: NetInfo? = null,
    val display: DisplayInfo? = null,
    val photos: Int = 0,
    val device: Identity? = null,
) {
    @Serializable data class Event(val at: String, val slot: String)
    @Serializable data class Shown(val content: String, val at: String)
    @Serializable data class TaskStatus(val name: String, val healthy: Boolean, val last_error: String? = null)
    @Serializable data class WeatherInfo(val fetched_at: String? = null, val warnings: List<Warning> = emptyList())
    @Serializable data class Warning(val title: String = "", val type_name: String = "", val color: String = "")
    @Serializable data class NetInfo(val online: Boolean = true)
    @Serializable data class DisplayInfo(val busy: Boolean = false, val last_refresh: String? = null, val last_error: String? = null)
    @Serializable data class Identity(val id: String, val name: String)
}

@Serializable
data class PhotoMeta(val id: String, val added: String, val favorite: Boolean = false, val paused: Boolean = false)

@Serializable
data class ConfigSaved(val saved: Boolean = false, val restart_required: Boolean = false)

/** A QWeather city lookup hit. */
data class City(val id: String, val name: String, val region: String)

class ApiException(message: String) : Exception(message)
