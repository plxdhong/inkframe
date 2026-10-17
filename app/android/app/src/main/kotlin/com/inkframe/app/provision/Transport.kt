package com.inkframe.app.provision

import java.io.Closeable

/** A connection to the device's provisioning service: BLE GATT, or the TCP simulator. */
interface Transport : Closeable {
    /** Largest fragment (header included) the link carries. */
    val maxFragment: Int

    suspend fun readInfo(): ByteArray

    /** Writes one fragment to RX; returns once the device acknowledged it. */
    suspend fun write(fragment: ByteArray)

    /** Next fragment notified on TX. Throws [TransportClosedException] once the link is gone. */
    suspend fun receive(): ByteArray
}

class TransportClosedException(message: String = "connection lost") : ProtocolException(message)
