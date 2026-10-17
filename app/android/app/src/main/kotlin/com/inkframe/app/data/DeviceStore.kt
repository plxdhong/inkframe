package com.inkframe.app.data

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.serialization.builtins.ListSerializer

private val Context.store by preferencesDataStore("devices")

/** Frames the user has set up, and which one is open. */
class DeviceStore(private val context: Context) {
    private val devicesKey = stringPreferencesKey("devices")
    private val currentKey = stringPreferencesKey("current")
    private val simKey = stringPreferencesKey("sim_host")
    private val serializer = ListSerializer(Device.serializer())

    val devices: Flow<List<Device>> = context.store.data.map { p ->
        p[devicesKey]?.let { runCatching { AppJson.decodeFromString(serializer, it) }.getOrNull() }.orEmpty()
    }

    val currentId: Flow<String?> = context.store.data.map { it[currentKey] }

    /** Developer setting: `host:port` of the daemon's provisioning simulator. */
    val simHost: Flow<String> = context.store.data.map { it[simKey] ?: "10.0.2.2:8090" }

    suspend fun setSimHost(v: String) {
        context.store.edit { it[simKey] = v.trim() }
    }

    /** Adds or updates (same id) a device and makes it current. */
    suspend fun save(device: Device) {
        context.store.edit { p ->
            val list = p[devicesKey]?.let { runCatching { AppJson.decodeFromString(serializer, it) }.getOrNull() }.orEmpty()
            p[devicesKey] = AppJson.encodeToString(serializer, list.filterNot { it.id == device.id } + device)
            p[currentKey] = device.id
        }
    }

    /** Follows a DHCP address change seen via mDNS. */
    suspend fun updateAddress(id: String, host: String, port: Int) {
        context.store.edit { p ->
            val list = p[devicesKey]?.let { runCatching { AppJson.decodeFromString(serializer, it) }.getOrNull() } ?: return@edit
            if (list.none { it.id == id && (it.host != host || it.port != port) }) return@edit
            p[devicesKey] = AppJson.encodeToString(serializer, list.map { if (it.id == id) it.copy(host = host, port = port) else it })
        }
    }

    suspend fun select(id: String) {
        context.store.edit { it[currentKey] = id }
    }

    suspend fun remove(id: String) {
        context.store.edit { p ->
            val list = p[devicesKey]?.let { runCatching { AppJson.decodeFromString(serializer, it) }.getOrNull() }.orEmpty()
            p[devicesKey] = AppJson.encodeToString(serializer, list.filterNot { it.id == id })
            if (p[currentKey] == id) p.remove(currentKey)
        }
    }
}
