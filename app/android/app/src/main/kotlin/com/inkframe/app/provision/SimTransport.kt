package com.inkframe.app.provision

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.io.DataInputStream
import java.io.DataOutputStream
import java.net.InetSocketAddress
import java.net.Socket
import kotlin.concurrent.thread

/**
 * The daemon's provisioning simulator (sim.rs): BLE fragments over TCP, for
 * developing without a radio. From the emulator the host is `10.0.2.2:8090`.
 * Records are `kind u8, len u16 BE, payload`.
 */
class SimTransport private constructor(private val socket: Socket) : Transport {
    override val maxFragment = 182
    private val out = DataOutputStream(socket.getOutputStream())
    private val notifications = Channel<ByteArray>(Channel.UNLIMITED)
    private val infos = Channel<ByteArray>(Channel.UNLIMITED)

    init {
        thread(name = "sim-reader", isDaemon = true) {
            val input = DataInputStream(socket.getInputStream())
            try {
                while (true) {
                    val kind = input.readUnsignedByte()
                    val payload = ByteArray(input.readUnsignedShort()).also { input.readFully(it) }
                    when (kind) {
                        KIND_INFO -> infos.trySend(payload)
                        KIND_NOTIFY -> notifications.trySend(payload)
                    }
                }
            } catch (_: Exception) {
            } finally {
                notifications.close()
                infos.close()
            }
        }
    }

    private suspend fun record(kind: Int, payload: ByteArray) = withContext(Dispatchers.IO) {
        synchronized(out) {
            out.writeByte(kind)
            out.writeShort(payload.size)
            out.write(payload)
            out.flush()
        }
    }

    override suspend fun readInfo(): ByteArray {
        record(KIND_INFO, ByteArray(0))
        return withTimeout(5_000) { infos.receiveCatching().getOrNull() ?: throw TransportClosedException() }
    }

    override suspend fun write(fragment: ByteArray) = record(KIND_WRITE, fragment)

    override suspend fun receive(): ByteArray =
        notifications.receiveCatching().getOrNull() ?: throw TransportClosedException()

    override fun close() = socket.close()

    companion object {
        const val KIND_INFO = 1
        const val KIND_WRITE = 2
        const val KIND_NOTIFY = 3

        suspend fun connect(host: String, port: Int): SimTransport = withContext(Dispatchers.IO) {
            val s = Socket()
            s.connect(InetSocketAddress(host, port), 5_000)
            s.tcpNoDelay = true
            SimTransport(s)
        }
    }
}
