package com.inkframe.app.ui.provision

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.inkframe.app.InkFrameApp
import com.inkframe.app.data.Device
import com.inkframe.app.provision.BleTransport
import com.inkframe.app.provision.DeviceInfo
import com.inkframe.app.provision.FailReason
import com.inkframe.app.provision.FoundFrame
import com.inkframe.app.provision.Network
import com.inkframe.app.provision.ProvisioningClient
import com.inkframe.app.provision.SimTransport
import com.inkframe.app.provision.Transport
import com.inkframe.app.provision.WifiState
import com.inkframe.app.provision.WrongCodeException
import com.inkframe.app.provision.scanFrames
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

sealed interface Step {
    /** Looking for frames over BLE. */
    data class Scan(val found: List<FoundFrame> = emptyList(), val scanning: Boolean = true) : Step
    data class Connecting(val name: String) : Step
    /** The frame draws the code when we say hello, which takes ~25 s on the panel. */
    data class Code(val info: DeviceInfo, val checking: Boolean = false) : Step
    data class Networks(val list: List<Network>, val loading: Boolean) : Step
    data class Password(val network: Network) : Step
    data class Joining(val ssid: String) : Step
    data class Joined(val device: Device, val reachable: Boolean?) : Step
}

data class ProvisionUi(val step: Step = Step.Scan(), val error: String? = null, val simHost: String = "")

class ProvisionViewModel(application: Application) : AndroidViewModel(application) {
    private val app = application as InkFrameApp
    private val _ui = MutableStateFlow(ProvisionUi())
    val ui: StateFlow<ProvisionUi> = _ui.asStateFlow()

    private var transport: Transport? = null
    private var client: ProvisioningClient? = null
    private var info: DeviceInfo? = null
    private var scanJob: Job? = null
    private var work: Job? = null

    init {
        viewModelScope.launch { _ui.update { it.copy(simHost = app.devices.simHost.first()) } }
    }

    private fun step(s: Step, error: String? = null) = _ui.update { it.copy(step = s, error = error) }

    private fun fail(e: Throwable, back: Step) = step(back, e.message ?: e.javaClass.simpleName)

    /** Runs one protocol action at a time. */
    private fun run(block: suspend () -> Unit) {
        if (work?.isActive == true) return
        work = viewModelScope.launch { block() }
    }

    fun startScan() {
        scanJob?.cancel()
        step(Step.Scan())
        scanJob = viewModelScope.launch {
            try {
                scanFrames(app).collect { f ->
                    _ui.update { u ->
                        val s = u.step as? Step.Scan ?: return@update u
                        u.copy(step = s.copy(found = (s.found.filterNot { it.device.address == f.device.address } + f).sortedByDescending { it.rssi }))
                    }
                }
            } catch (e: Exception) {
                _ui.update { it.copy(step = Step.Scan(scanning = false), error = e.message) }
            }
        }
    }

    fun stopScan() {
        scanJob?.cancel()
        _ui.update { u -> (u.step as? Step.Scan)?.let { u.copy(step = it.copy(scanning = false)) } ?: u }
    }

    fun connectBle(frame: FoundFrame) = connect(frame.name) { BleTransport.connect(app, frame.device) }

    fun connectSim(hostPort: String) {
        viewModelScope.launch { app.devices.setSimHost(hostPort) }
        _ui.update { it.copy(simHost = hostPort) }
        val host = hostPort.substringBeforeLast(':')
        val port = hostPort.substringAfterLast(':').toIntOrNull() ?: 8090
        connect("模拟器 $hostPort") { SimTransport.connect(host, port) }
    }

    private fun connect(name: String, open: suspend () -> Transport) = run {
        stopScan()
        step(Step.Connecting(name))
        try {
            close()
            val t = open()
            transport = t
            val c = ProvisioningClient(t)
            client = c
            val i = c.info()
            info = i
            c.hello()
            step(Step.Code(i))
        } catch (e: Exception) {
            close()
            fail(e, Step.Scan(scanning = false))
        }
    }

    fun submitCode(code: String) = run {
        val c = client ?: return@run
        val i = info ?: return@run
        step(Step.Code(i, checking = true))
        try {
            c.authenticate(code)
            step(Step.Networks(emptyList(), loading = true))
            loadNetworks()
        } catch (e: WrongCodeException) {
            step(Step.Code(i), "配网码不对，请对照相框上的显示重新输入")
        } catch (e: Exception) {
            fail(e, Step.Code(i))
        }
    }

    fun rescan() = run {
        step(Step.Networks((ui.value.step as? Step.Networks)?.list.orEmpty(), loading = true))
        loadNetworks()
    }

    private suspend fun loadNetworks() {
        val c = client ?: return
        try {
            step(Step.Networks(c.scan(), loading = false))
        } catch (e: Exception) {
            fail(e, Step.Networks(emptyList(), loading = false))
        }
    }

    fun choose(network: Network) {
        if (network.secure) step(Step.Password(network)) else join(network, "")
    }

    fun chooseHidden(ssid: String) = step(Step.Password(Network(ssid, 0, secure = true)))

    fun backToNetworks() = step(Step.Networks((ui.value.step as? Step.Networks)?.list.orEmpty(), loading = false))

    fun join(network: Network, psk: String, hidden: Boolean = network.signal == 0) = run {
        val c = client ?: return@run
        step(Step.Joining(network.ssid))
        try {
            var final: WifiState? = null
            c.connect(network.ssid, psk, hidden).collect { final = it }
            when (val s = final) {
                is WifiState.Connected -> finish(s)
                is WifiState.Failed -> step(
                    if (s.reason == FailReason.AUTH_FAILED) Step.Password(network) else Step.Networks(emptyList(), loading = false),
                    when (s.reason) {
                        FailReason.AUTH_FAILED -> "密码不正确"
                        FailReason.NOT_FOUND -> "相框附近找不到「${s.ssid}」"
                        FailReason.TIMEOUT -> "连接超时，请靠近路由器后重试"
                        FailReason.ERROR -> "连接失败：${s.detail}"
                    },
                )
                else -> step(Step.Networks(emptyList(), loading = false), "相框没有返回结果")
            }
        } catch (e: Exception) {
            fail(e, Step.Password(network))
        }
    }

    private suspend fun finish(s: WifiState.Connected) {
        val i = info ?: return
        val device = Device(id = i.id, name = i.name, host = s.ip.ifEmpty { s.host }, port = s.port)
        app.devices.save(device)
        step(Step.Joined(device, reachable = null))
        // Ends provisioning on the frame, which then repaints its schedule.
        runCatching { client?.done() }
        close()
        // The phone may still be on another network; try the API a few times.
        var ok = false
        repeat(5) {
            if (!ok) {
                ok = runCatching { app.api(device).status() }.isSuccess
                if (!ok) delay(2_000)
            }
        }
        step(Step.Joined(device, reachable = ok))
    }

    fun dismissError() = _ui.update { it.copy(error = null) }

    private fun close() {
        runCatching { transport?.close() }
        transport = null
        client = null
    }

    override fun onCleared() {
        close()
    }
}
