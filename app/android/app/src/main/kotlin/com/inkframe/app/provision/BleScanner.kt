package com.inkframe.app.provision

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.os.ParcelUuid
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow

data class FoundFrame(val device: BluetoothDevice, val name: String, val rssi: Int)

/** Scans for frames advertising the provisioning service. Needs BLUETOOTH_SCAN. */
@SuppressLint("MissingPermission")
fun scanFrames(context: Context): Flow<FoundFrame> = callbackFlow {
    val adapter = context.getSystemService(BluetoothManager::class.java)?.adapter
    val scanner = adapter?.bluetoothLeScanner
    if (adapter == null || !adapter.isEnabled || scanner == null) {
        close(ProtocolException("请先打开蓝牙"))
        return@callbackFlow
    }
    val cb = object : ScanCallback() {
        override fun onScanResult(type: Int, r: ScanResult) {
            val name = r.scanRecord?.deviceName ?: r.device.name ?: r.device.address
            trySend(FoundFrame(r.device, name, r.rssi))
        }

        override fun onScanFailed(errorCode: Int) {
            close(ProtocolException("蓝牙扫描失败 ($errorCode)"))
        }
    }
    val filter = ScanFilter.Builder().setServiceUuid(ParcelUuid.fromString(Gatt.SERVICE)).build()
    val settings = ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build()
    scanner.startScan(listOf(filter), settings, cb)
    awaitClose { runCatching { scanner.stopScan(cb) } }
}
