package com.inkframe.app.ui.provision

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Bluetooth
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.inkframe.app.BuildConfig
import com.inkframe.app.data.Device
import com.inkframe.app.provision.Network
import com.inkframe.app.provision.SetupCode
import com.inkframe.app.ui.ErrorBanner
import com.inkframe.app.ui.InkCard
import com.inkframe.app.ui.theme.Ink

private val blePermissions =
    if (Build.VERSION.SDK_INT >= 31) arrayOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT)
    else arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProvisionScreen(onBack: () -> Unit, onDone: (Device) -> Unit, vm: ProvisionViewModel = viewModel()) {
    val ui by vm.ui.collectAsStateWithLifecycle()
    val context = LocalContext.current
    var granted by remember {
        mutableStateOf(blePermissions.all { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED })
    }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { r ->
        granted = r.values.all { it }
        if (granted) vm.startScan()
    }
    LaunchedEffect(Unit) { if (granted) vm.startScan() else ask.launch(blePermissions) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("添加相框") },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "返回") } },
            )
        },
    ) { pad ->
        Column(Modifier.padding(pad).fillMaxSize().padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            StepHeader(ui.step)
            ui.error?.let { ErrorBanner(it) }
            when (val s = ui.step) {
                is Step.Scan -> ScanStep(s, granted, ui.simHost, onGrant = { ask.launch(blePermissions) }, vm)
                is Step.Connecting -> Busy("正在连接 ${s.name}…")
                is Step.Code -> CodeStep(s, vm::submitCode)
                is Step.Networks -> NetworksStep(s, vm)
                is Step.Password -> PasswordStep(s.network, onBack = vm::backToNetworks, onJoin = { vm.join(s.network, it) })
                is Step.Joining -> Busy("相框正在连接「${s.ssid}」，最多需要一分钟…")
                is Step.Joined -> JoinedStep(s, onDone)
            }
        }
    }
}

@Composable
private fun StepHeader(step: Step) {
    val index = when (step) {
        is Step.Scan, is Step.Connecting -> 0
        is Step.Code -> 1
        is Step.Networks, is Step.Password, is Step.Joining -> 2
        is Step.Joined -> 3
    }
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.fillMaxWidth()) {
        listOf("找到相框", "输入配网码", "选择 Wi-Fi", "完成").forEachIndexed { i, label ->
            Column(Modifier.weight(1f)) {
                Box(
                    Modifier.fillMaxWidth().height(6.dp)
                        .background(if (i <= index) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant),
                )
                Text(label, style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(top = 4.dp))
            }
        }
    }
}

@Composable
private fun Busy(text: String) {
    Column(Modifier.fillMaxWidth().padding(top = 48.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        CircularProgressIndicator()
        Spacer(Modifier.height(16.dp))
        Text(text, style = MaterialTheme.typography.bodyLarge)
    }
}

@Composable
private fun ColumnScope.ScanStep(s: Step.Scan, granted: Boolean, simHost: String, onGrant: () -> Unit, vm: ProvisionViewModel) {
    if (!granted) {
        InkCard(title = "需要蓝牙权限") {
            Text("用蓝牙找到附近的相框并发送 Wi-Fi 信息。")
            Button(onClick = onGrant) { Text("授予权限") }
        }
    } else {
        Text("让相框通电并靠近手机。新相框开机后会自动进入配网；已联网的相框可在调试页点「开始配网」。", style = MaterialTheme.typography.bodyMedium)
        if (s.scanning) LinearProgressIndicator(Modifier.fillMaxWidth())
        if (s.found.isEmpty() && !s.scanning) OutlinedButton(onClick = vm::startScan) { Text("重新搜索") }
        LazyColumn(Modifier.weight(1f, fill = false)) {
            items(s.found, key = { it.device.address }) { f ->
                ListItem(
                    headlineContent = { Text(f.name, fontWeight = FontWeight.Bold) },
                    supportingContent = { Text("信号 ${f.rssi} dBm") },
                    leadingContent = { Icon(Icons.Default.Bluetooth, null) },
                    modifier = Modifier.clickable { vm.connectBle(f) },
                )
                HorizontalDivider()
            }
        }
    }
    if (BuildConfig.DEBUG) SimulatorCard(simHost, vm::connectSim)
}

/** Development only: the daemon's TCP stand-in for BLE (provision.transport = "sim"). */
@Composable
private fun SimulatorCard(initial: String, onConnect: (String) -> Unit) {
    var host by rememberSaveable(initial) { mutableStateOf(initial) }
    InkCard(title = "开发：模拟器") {
        Text("连接 inkframe 守护进程的配网模拟器，无需真实蓝牙。模拟器里所有网络的密码都是 inkframe123。", style = MaterialTheme.typography.bodySmall)
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(host, { host = it }, singleLine = true, label = { Text("host:port") }, modifier = Modifier.weight(1f))
            Spacer(Modifier.width(8.dp))
            OutlinedButton(onClick = { onConnect(host) }) { Text("连接") }
        }
    }
}

@Composable
private fun CodeStep(s: Step.Code, onSubmit: (String) -> Unit) {
    var code by rememberSaveable { mutableStateOf("") }
    val normalized = SetupCode.normalize(code)
    InkCard(title = "已连接 ${s.info.name}") {
        Text("相框屏幕正在显示 12 位配网码（墨水屏刷新约 25 秒）。输入时不区分大小写，短横线可省略。")
        OutlinedTextField(
            value = code,
            onValueChange = { code = it.uppercase().filter { c -> c.isLetterOrDigit() || c == '-' || c == ' ' }.take(16) },
            label = { Text("配网码") },
            placeholder = { Text("XXXX-XXXX-XXXX") },
            singleLine = true,
            textStyle = MaterialTheme.typography.headlineSmall.copy(fontFamily = FontFamily.Monospace, letterSpacing = 2.sp),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Characters, keyboardType = KeyboardType.Ascii, imeAction = ImeAction.Done, autoCorrectEnabled = false),
            keyboardActions = KeyboardActions(onDone = { normalized?.let(onSubmit) }),
            modifier = Modifier.fillMaxWidth(),
            enabled = !s.checking,
        )
        Button(onClick = { normalized?.let(onSubmit) }, enabled = normalized != null && !s.checking, modifier = Modifier.fillMaxWidth()) {
            if (s.checking) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp) else Text("验证")
        }
    }
}

@Composable
private fun ColumnScope.NetworksStep(s: Step.Networks, vm: ProvisionViewModel) {
    var hidden by rememberSaveable { mutableStateOf("") }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("相框附近的 Wi-Fi", style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
        IconButton(onClick = vm::rescan, enabled = !s.loading) { Icon(Icons.Default.Refresh, "重新扫描") }
    }
    if (s.loading) LinearProgressIndicator(Modifier.fillMaxWidth())
    LazyColumn(Modifier.weight(1f, fill = false)) {
        items(s.list, key = { it.ssid }) { n -> NetworkRow(n) { vm.choose(n) } }
        item {
            Column(Modifier.padding(vertical = 12.dp)) {
                Text("隐藏网络", style = MaterialTheme.typography.labelLarge)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(hidden, { hidden = it }, singleLine = true, label = { Text("网络名称 (SSID)") }, modifier = Modifier.weight(1f))
                    Spacer(Modifier.width(8.dp))
                    OutlinedButton(onClick = { vm.chooseHidden(hidden.trim()) }, enabled = hidden.isNotBlank()) { Text("下一步") }
                }
            }
        }
    }
}

@Composable
private fun NetworkRow(n: Network, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(n.ssid) },
        supportingContent = { Text(if (n.signal >= 60) "信号强" else if (n.signal >= 35) "信号中等" else "信号弱") },
        leadingContent = { Icon(Icons.Default.Wifi, null, tint = if (n.signal >= 35) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant) },
        trailingContent = { if (n.secure) Icon(Icons.Default.Lock, "需要密码") },
        modifier = Modifier.clickable(onClick = onClick),
    )
    HorizontalDivider()
}

@Composable
private fun PasswordStep(network: Network, onBack: () -> Unit, onJoin: (String) -> Unit) {
    var psk by rememberSaveable(network.ssid) { mutableStateOf("") }
    var show by remember { mutableStateOf(false) }
    InkCard(title = "连接「${network.ssid}」") {
        OutlinedTextField(
            value = psk,
            onValueChange = { psk = it },
            label = { Text("Wi-Fi 密码") },
            singleLine = true,
            visualTransformation = if (show) VisualTransformation.None else PasswordVisualTransformation(),
            trailingIcon = {
                IconButton(onClick = { show = !show }) { Icon(if (show) Icons.Default.VisibilityOff else Icons.Default.Visibility, "显示密码") }
            },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Done, autoCorrectEnabled = false),
            keyboardActions = KeyboardActions(onDone = { if (psk.length >= 8) onJoin(psk) }),
            modifier = Modifier.fillMaxWidth(),
        )
        Text("密码经加密后通过蓝牙发送给相框，不经过任何服务器。", style = MaterialTheme.typography.bodySmall)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onBack) { Text("换一个网络") }
            Button(onClick = { onJoin(psk) }, enabled = psk.length >= 8) { Text("连接") }
        }
    }
}

@Composable
private fun JoinedStep(s: Step.Joined, onDone: (Device) -> Unit) {
    InkCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Default.CheckCircle, null, tint = Ink.Green, modifier = Modifier.size(32.dp))
            Spacer(Modifier.width(12.dp))
            Text("${s.device.name} 已联网", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Black)
        }
        Text("地址 ${s.device.host}:${s.device.port}")
        when (s.reachable) {
            null -> Row(verticalAlignment = Alignment.CenterVertically) {
                CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                Spacer(Modifier.width(8.dp))
                Text("正在通过局域网连接相框…")
            }
            true -> Text("手机已能访问相框。")
            false -> Text("手机暂时访问不到相框。请确认手机连的是同一个 Wi-Fi；之后会通过局域网自动发现。", color = MaterialTheme.colorScheme.error)
        }
        Button(onClick = { onDone(s.device) }, enabled = s.reachable != null, modifier = Modifier.fillMaxWidth()) { Text("开始使用") }
    }
}
