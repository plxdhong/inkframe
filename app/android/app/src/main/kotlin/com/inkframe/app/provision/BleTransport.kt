package com.inkframe.app.provision

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.content.Context
import android.os.Build
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeout
import java.util.UUID

/**
 * GATT client for the provisioning service. Android allows one outstanding
 * GATT operation at a time, so every operation goes through [op], which waits
 * for its callback with a timeout. Callers must hold BLUETOOTH_CONNECT.
 */
@SuppressLint("MissingPermission")
class BleTransport private constructor(private val context: Context, private val device: BluetoothDevice) : Transport {
    private lateinit var gatt: BluetoothGatt
    private lateinit var info: BluetoothGattCharacteristic
    private lateinit var rx: BluetoothGattCharacteristic
    private lateinit var tx: BluetoothGattCharacteristic

    private val lock = Mutex()
    @Volatile private var pending: CompletableDeferred<Any?>? = null
    private val connected = CompletableDeferred<Unit>()
    private val notifications = Channel<ByteArray>(Channel.UNLIMITED)

    @Volatile override var maxFragment: Int = Frames.MIN_CHUNK + 1
        private set

    private fun complete(value: Any?, ok: Boolean, what: String) {
        val p = pending ?: return
        if (ok) p.complete(value) else p.completeExceptionally(ProtocolException("$what failed"))
    }

    private val callback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(g: BluetoothGatt, status: Int, newState: Int) {
            if (newState == BluetoothProfile.STATE_CONNECTED && status == BluetoothGatt.GATT_SUCCESS) {
                connected.complete(Unit)
            } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                val e = TransportClosedException(if (status == BluetoothGatt.GATT_SUCCESS) "已断开" else "蓝牙连接失败 ($status)")
                connected.completeExceptionally(e)
                pending?.completeExceptionally(e)
                notifications.close(e)
                g.close()
            }
        }

        override fun onMtuChanged(g: BluetoothGatt, mtu: Int, status: Int) {
            if (status == BluetoothGatt.GATT_SUCCESS) maxFragment = mtu - 3
            complete(mtu, true, "mtu")
        }

        override fun onServicesDiscovered(g: BluetoothGatt, status: Int) =
            complete(null, status == BluetoothGatt.GATT_SUCCESS, "service discovery")

        override fun onCharacteristicRead(g: BluetoothGatt, c: BluetoothGattCharacteristic, value: ByteArray, status: Int) =
            complete(value, status == BluetoothGatt.GATT_SUCCESS, "read")

        @Deprecated("API < 33")
        override fun onCharacteristicRead(g: BluetoothGatt, c: BluetoothGattCharacteristic, status: Int) {
            @Suppress("DEPRECATION")
            complete(c.value, status == BluetoothGatt.GATT_SUCCESS, "read")
        }

        override fun onCharacteristicWrite(g: BluetoothGatt, c: BluetoothGattCharacteristic, status: Int) =
            complete(null, status == BluetoothGatt.GATT_SUCCESS, "write")

        override fun onDescriptorWrite(g: BluetoothGatt, d: BluetoothGattDescriptor, status: Int) =
            complete(null, status == BluetoothGatt.GATT_SUCCESS, "subscribe")

        override fun onCharacteristicChanged(g: BluetoothGatt, c: BluetoothGattCharacteristic, value: ByteArray) {
            if (::tx.isInitialized && c.uuid == tx.uuid) notifications.trySend(value)
        }

        @Deprecated("API < 33")
        override fun onCharacteristicChanged(g: BluetoothGatt, c: BluetoothGattCharacteristic) {
            @Suppress("DEPRECATION")
            if (Build.VERSION.SDK_INT < 33 && ::tx.isInitialized && c.uuid == tx.uuid) notifications.trySend(c.value)
        }
    }

    /** Runs one GATT operation and waits for its callback. */
    private suspend fun <T> op(timeoutMs: Long = 10_000, start: () -> Boolean): T = lock.withLock {
        val d = CompletableDeferred<Any?>()
        pending = d
        try {
            if (!start()) throw ProtocolException("蓝牙操作被拒绝")
            @Suppress("UNCHECKED_CAST")
            withTimeout(timeoutMs) { d.await() } as T
        } finally {
            pending = null
        }
    }

    private suspend fun open() {
        // API 37 adds a settings-based overload; this one covers minSdk 29 and up.
        @Suppress("DEPRECATION")
        gatt = device.connectGatt(context, false, callback, BluetoothDevice.TRANSPORT_LE)
        withTimeout(15_000) { connected.await() }
        // Larger MTU = fewer fragments; failure is fine (we keep the minimum).
        runCatching { op<Int> { gatt.requestMtu(247) } }
        op<Unit?>(15_000) { gatt.discoverServices() }
        val service = gatt.getService(UUID.fromString(Gatt.SERVICE)) ?: throw ProtocolException("这不是 InkFrame 相框")
        fun char(id: String) = service.getCharacteristic(UUID.fromString(id)) ?: throw ProtocolException("缺少特征 $id")
        info = char(Gatt.INFO)
        rx = char(Gatt.RX)
        tx = char(Gatt.TX)
        gatt.setCharacteristicNotification(tx, true)
        val cccd = tx.getDescriptor(UUID.fromString(Gatt.CCCD)) ?: throw ProtocolException("TX 不支持通知")
        op<Unit?> {
            if (Build.VERSION.SDK_INT >= 33) {
                gatt.writeDescriptor(cccd, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE) == BluetoothStatusCodes.SUCCESS
            } else {
                @Suppress("DEPRECATION")
                cccd.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                @Suppress("DEPRECATION")
                gatt.writeDescriptor(cccd)
            }
        }
    }

    override suspend fun readInfo(): ByteArray = op { gatt.readCharacteristic(info) }

    override suspend fun write(fragment: ByteArray) {
        op<Unit?> {
            if (Build.VERSION.SDK_INT >= 33) {
                gatt.writeCharacteristic(rx, fragment, BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT) == BluetoothStatusCodes.SUCCESS
            } else {
                @Suppress("DEPRECATION")
                rx.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                @Suppress("DEPRECATION")
                rx.value = fragment
                @Suppress("DEPRECATION")
                gatt.writeCharacteristic(rx)
            }
        }
    }

    override suspend fun receive(): ByteArray =
        notifications.receiveCatching().getOrNull() ?: throw TransportClosedException()

    override fun close() {
        if (::gatt.isInitialized) {
            gatt.disconnect()
            gatt.close()
        }
        notifications.close()
    }

    companion object {
        suspend fun connect(context: Context, device: BluetoothDevice): BleTransport {
            val t = BleTransport(context.applicationContext, device)
            try {
                t.open()
            } catch (e: Throwable) {
                t.close()
                throw e
            }
            return t
        }
    }
}
