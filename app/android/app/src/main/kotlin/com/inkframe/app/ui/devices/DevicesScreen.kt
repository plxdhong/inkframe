package com.inkframe.app.ui.devices

import android.app.Application
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material.icons.outlined.PhotoSizeSelectActual
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.inkframe.app.InkFrameApp
import com.inkframe.app.data.Device
import com.inkframe.app.data.discoverFrames
import com.inkframe.app.ui.ErrorBanner
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.onEach
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

class DevicesViewModel(application: Application) : AndroidViewModel(application) {
    private val app = application as InkFrameApp
    val saved: StateFlow<List<Device>> = app.devices.devices.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    /** Frames on the LAN; known ones get their address refreshed (DHCP may move them). */
    val nearby: StateFlow<List<Device>> = discoverFrames(app)
        .onEach { list -> list.forEach { app.devices.updateAddress(it.id, it.host, it.port) } }
        .catch { emit(emptyList()) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    val error = MutableStateFlow<String?>(null)

    fun open(d: Device, then: (Device) -> Unit) = viewModelScope.launch {
        app.devices.save(d)
        then(d)
    }

    fun forget(d: Device) = viewModelScope.launch { app.devices.remove(d.id) }

    /** Adds a frame by address (e.g. over the USB link at 10.12.0.1). */
    fun addManual(hostPort: String, then: (Device) -> Unit) = viewModelScope.launch {
        val host = hostPort.substringBeforeLast(':').trim()
        val port = hostPort.substringAfterLast(':', "8080").toIntOrNull() ?: 8080
        val probe = Device("?", host, host, port)
        runCatching { app.api(probe).status() }
            .onSuccess { s ->
                val id = s.device?.id ?: host
                val d = Device(id, s.device?.name ?: host, host, port)
                app.devices.save(d)
                error.value = null
                then(d)
            }
            .onFailure { error.value = "无法连接 $host:$port：${it.message}" }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DevicesScreen(onOpen: (Device) -> Unit, onAdd: () -> Unit, vm: DevicesViewModel = viewModel()) {
    val saved by vm.saved.collectAsStateWithLifecycle()
    val nearby by vm.nearby.collectAsStateWithLifecycle()
    val error by vm.error.collectAsStateWithLifecycle()
    var manual by rememberSaveable { mutableStateOf(false) }
    val newNearby = nearby.filter { n -> saved.none { it.id == n.id } }

    Scaffold(
        topBar = { TopAppBar(title = { Text("我的相框", fontWeight = FontWeight.Black) }) },
        floatingActionButton = {
            ExtendedFloatingActionButton(onClick = onAdd, icon = { Icon(Icons.Default.Add, null) }, text = { Text("添加相框") })
        },
    ) { pad ->
        LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = androidx.compose.foundation.layout.PaddingValues(bottom = 96.dp)) {
            error?.let { item { ErrorBanner(it, Modifier.padding(16.dp)) } }
            if (saved.isEmpty() && newNearby.isEmpty()) {
                item {
                    Column(Modifier.fillMaxWidth().padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Icon(Icons.Outlined.PhotoSizeSelectActual, null, Modifier.padding(8.dp))
                        Text("还没有相框", style = MaterialTheme.typography.titleMedium)
                        Text("点「添加相框」，用蓝牙给新相框配网。", style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
            if (saved.isNotEmpty()) item { Section("已添加") }
            items(saved, key = { "s-" + it.id }) { d ->
                val online = nearby.any { it.id == d.id }
                ListItem(
                    headlineContent = { Text(d.name, fontWeight = FontWeight.Bold) },
                    supportingContent = { Text("${d.host}:${d.port}" + if (online) " · 在局域网中" else "") },
                    leadingContent = { Icon(Icons.Outlined.PhotoSizeSelectActual, null) },
                    trailingContent = { IconButton(onClick = { vm.forget(d) }) { Icon(Icons.Default.Delete, "移除") } },
                    modifier = Modifier.clickable { vm.open(d, onOpen) },
                )
                HorizontalDivider()
            }
            if (newNearby.isNotEmpty()) item { Section("局域网中发现") }
            items(newNearby, key = { "n-" + it.id }) { d ->
                ListItem(
                    headlineContent = { Text(d.name) },
                    supportingContent = { Text("${d.host}:${d.port}") },
                    leadingContent = { Icon(Icons.Default.Wifi, null) },
                    modifier = Modifier.clickable { vm.open(d, onOpen) },
                )
                HorizontalDivider()
            }
            item {
                TextButton(onClick = { manual = true }, modifier = Modifier.padding(8.dp)) { Text("按地址添加（如 USB 调试口 10.12.0.1）") }
            }
        }
    }
    if (manual) ManualDialog(onDismiss = { manual = false }) { manual = false; vm.addManual(it, onOpen) }
}

@Composable
private fun Section(text: String) {
    Text(text, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp))
}

@Composable
private fun ManualDialog(onDismiss: () -> Unit, onAdd: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf("10.12.0.1:8080") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("按地址添加") },
        text = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(text, { text = it }, singleLine = true, label = { Text("地址:端口") })
                Spacer(Modifier.width(4.dp))
            }
        },
        confirmButton = { TextButton(onClick = { onAdd(text) }) { Text("添加") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("取消") } },
    )
}
