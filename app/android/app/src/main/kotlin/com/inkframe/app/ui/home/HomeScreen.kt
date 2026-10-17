package com.inkframe.app.ui.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewModelScope
import coil3.compose.SubcomposeAsyncImage
import com.inkframe.app.data.InkFrameApi
import com.inkframe.app.data.Status
import com.inkframe.app.ui.ErrorBanner
import com.inkframe.app.ui.InkCard
import com.inkframe.app.ui.PanelFrame
import com.inkframe.app.ui.StatusDot
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class HomeUi(val status: Status? = null, val error: String? = null, val bust: Long = 0, val busy: Boolean = false)

class HomeViewModel(val api: InkFrameApi) : ViewModel() {
    private val _ui = MutableStateFlow(HomeUi())
    val ui = _ui.asStateFlow()

    suspend fun poll() {
        while (true) {
            load()
            delay(10_000)
        }
    }

    private suspend fun load() {
        runCatching { api.status() }
            .onSuccess { s ->
                _ui.update {
                    // Reload the preview whenever the panel finished another refresh.
                    val changed = it.status?.display?.last_refresh != s.display?.last_refresh
                    it.copy(status = s, error = null, bust = if (changed) System.currentTimeMillis() else it.bust)
                }
            }
            .onFailure { e -> _ui.update { it.copy(error = "连不上相框：${e.message}") } }
    }

    private fun act(block: suspend () -> Unit) = viewModelScope.launch {
        _ui.update { it.copy(busy = true) }
        runCatching { block() }.onFailure { e -> _ui.update { it.copy(error = e.message) } }
        load()
        _ui.update { it.copy(busy = false) }
    }

    fun refresh(target: String) = act { api.refresh(target) }
    fun setMode(mode: String) = act { api.setMode(mode) }
}

private val slotNames = mapOf("morning" to "早间仪表盘", "evening" to "晚间仪表盘", "photo" to "照片")

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun HomeScreen(vm: HomeViewModel) {
    val ui by vm.ui.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(vm) { lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { vm.poll() } }
    val s = ui.status

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        ui.error?.let { ErrorBanner(it) }
        PanelFrame {
            SubcomposeAsyncImage(
                model = vm.api.previewUrl(ui.bust),
                contentDescription = "相框当前画面",
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize(),
                loading = { CircularProgressIndicator(Modifier.padding(48.dp)) },
                error = { Text("还没有画面", color = MaterialTheme.colorScheme.onSurfaceVariant) },
            )
        }
        if (s?.display?.busy == true) Text("墨水屏正在刷新（约 25 秒）…", style = MaterialTheme.typography.bodySmall)

        InkCard(title = "立即显示") {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = { vm.refresh("auto") }, enabled = !ui.busy) { Text("按时间表刷新") }
                OutlinedButton(onClick = { vm.refresh("morning") }, enabled = !ui.busy) { Text("早间仪表盘") }
                OutlinedButton(onClick = { vm.refresh("evening") }, enabled = !ui.busy) { Text("晚间仪表盘") }
                OutlinedButton(onClick = { vm.refresh("photo") }, enabled = !ui.busy) { Text("下一张照片") }
            }
            Text("模式", style = MaterialTheme.typography.labelLarge)
            val modes = listOf("auto" to "按时间表", "dashboard" to "仅仪表盘", "photo" to "仅照片")
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                modes.forEachIndexed { i, (key, label) ->
                    SegmentedButton(
                        selected = s?.mode == key,
                        onClick = { vm.setMode(key) },
                        shape = SegmentedButtonDefaults.itemShape(i, modes.size),
                        enabled = !ui.busy,
                    ) { Text(label) }
                }
            }
        }

        if (s != null) {
            InkCard(title = "状态") {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    StatusDot(s.healthy)
                    Spacer(Modifier.width(8.dp))
                    Text(if (s.healthy) "运行正常" else "有任务异常：" + s.tasks.filterNot { it.healthy }.joinToString { it.name })
                }
                Kv("现在", "${s.now} · ${if (s.workday) "工作日" else "休息日"}")
                s.shown?.let { Kv("正在显示", it.content.substringBefore(':').let { c -> slotNames[c.removePrefix("dashboard-")] ?: c }) }
                s.next_event?.let { Kv("下一次", "${it.at.replace('T', ' ').take(16)} ${slotNames[it.slot] ?: it.slot}") }
                Kv("网络", if (s.network?.online != false) "在线" else "离线（显示缓存数据）")
                s.weather?.warnings?.takeIf { it.isNotEmpty() }?.let { w -> Kv("预警", w.joinToString { it.title.ifEmpty { it.type_name } }) }
                Kv("照片", "${s.photos} 张")
                Kv("版本", s.version)
                s.display?.last_error?.let { Kv("屏幕错误", it) }
            }
        }
    }
}

@Composable
private fun Kv(k: String, v: String) {
    Row {
        Text(k, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.width(72.dp))
        Text(v, style = MaterialTheme.typography.bodyMedium)
    }
}
