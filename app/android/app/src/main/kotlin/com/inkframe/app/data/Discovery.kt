package com.inkframe.app.data

import android.content.Context
import android.os.Build
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull
import java.net.Inet4Address
import java.net.InetAddress

/** Frames advertising `_inkframe._tcp` on the LAN (mdns.rs). */
fun discoverFrames(context: Context): Flow<List<Device>> = callbackFlow {
    val nsd = context.getSystemService(NsdManager::class.java)
    val found = linkedMapOf<String, Device>()
    // NsdManager resolves one service at a time.
    val resolving = Mutex()

    suspend fun resolve(info: NsdServiceInfo): NsdServiceInfo? = resolving.withLock {
        val done = CompletableDeferred<NsdServiceInfo?>()
        @Suppress("DEPRECATION")
        nsd.resolveService(info, object : NsdManager.ResolveListener {
            override fun onResolveFailed(s: NsdServiceInfo, code: Int) { done.complete(null) }
            override fun onServiceResolved(s: NsdServiceInfo) { done.complete(s) }
        })
        withTimeoutOrNull(5_000) { done.await() }
    }

    val listener = object : NsdManager.DiscoveryListener {
        override fun onDiscoveryStarted(type: String) {}
        override fun onDiscoveryStopped(type: String) {}
        override fun onStartDiscoveryFailed(type: String, code: Int) { close() }
        override fun onStopDiscoveryFailed(type: String, code: Int) {}

        override fun onServiceFound(info: NsdServiceInfo) {
            launch {
                val r = resolve(info) ?: return@launch
                val addrs = if (Build.VERSION.SDK_INT >= 34) r.hostAddresses else @Suppress("DEPRECATION") listOfNotNull(r.host)
                val host = pickHost(addrs) ?: return@launch
                val id = r.attributes["id"]?.decodeToString() ?: r.serviceName.substringAfterLast('-')
                found[r.serviceName] = Device(id = id, name = r.serviceName, host = host, port = r.port)
                trySend(found.values.toList())
            }
        }

        override fun onServiceLost(info: NsdServiceInfo) {
            if (found.remove(info.serviceName) != null) trySend(found.values.toList())
        }
    }
    trySend(emptyList())
    nsd.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
    awaitClose { runCatching { nsd.stopServiceDiscovery(listener) } }
}

/**
 * NSD can resolve to an IPv6 link-local address (`fe80::…%wlan0`), which a URL
 * can't carry and which would overwrite a saved, working address. Prefer IPv4.
 */
internal fun pickHost(addrs: List<InetAddress>): String? {
    val usable = addrs.filterNot { it.isLinkLocalAddress || it.isLoopbackAddress }
    return (usable.firstOrNull { it is Inet4Address } ?: usable.firstOrNull())?.hostAddress
}

private const val SERVICE_TYPE = "_inkframe._tcp"
