package com.inkframe.app.ui.settings

import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.InputChip
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import com.inkframe.app.data.City
import com.inkframe.app.data.DayPlan
import com.inkframe.app.data.InkFrameApi
import com.inkframe.app.data.SettingsForm
import com.inkframe.app.data.Times
import com.inkframe.app.ui.ErrorBanner
import com.inkframe.app.ui.InkCard
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlin.math.roundToInt

data class SettingsUi(
    val form: SettingsForm? = null,
    val saved: SettingsForm? = null,
    val error: String? = null,
    val note: String? = null,
    val saving: Boolean = false,
    val cities: List<City> = emptyList(),
)

class SettingsViewModel(private val api: InkFrameApi) : ViewModel() {
    private val _ui = MutableStateFlow(SettingsUi())
    val ui = _ui.asStateFlow()
    private var raw: JsonObject? = null

    fun load() = viewModelScope.launch {
        runCatching { api.config() }
            .onSuccess { cfg ->
                raw = cfg
                val f = SettingsForm.read(cfg)
                _ui.update { it.copy(form = f, saved = f, error = null) }
            }
            .onFailure { e -> _ui.update { it.copy(error = "读取设置失败：${e.message}") } }
    }

    fun edit(f: SettingsForm) = _ui.update { it.copy(form = f, note = null) }

    fun save() = viewModelScope.launch {
        val f = _ui.value.form ?: return@launch
        val cfg = raw ?: return@launch
        val errors = f.errors()
        if (errors.isNotEmpty()) {
            _ui.update { it.copy(error = errors.joinToString("\n")) }
            return@launch
        }
        _ui.update { it.copy(saving = true, error = null) }
        val enhanceChanged = _ui.value.saved?.let { s -> s.saturation != f.saturation || s.contrast != f.contrast || s.brightness != f.brightness } ?: false
        runCatching {
            val out = f.writeTo(cfg)
            val r = api.saveConfig(out)
            raw = out
            if (enhanceChanged) api.reprocessPhotos()
            r
        }
            .onSuccess { r ->
                val note = buildString {
                    append("已保存")
                    if (enhanceChanged) append("，照片已按新参数重新处理")
                    if (r.restart_required) append("。部分设置需重启相框后生效")
                }
                _ui.update { it.copy(saving = false, saved = f, note = note) }
            }
            .onFailure { e -> _ui.update { it.copy(saving = false, error = "保存失败：${e.message}") } }
    }

    fun searchCity(q: String) = viewModelScope.launch {
        val f = _ui.value.form ?: return@launch
        if (f.apiHost.isBlank() || f.apiKey.isBlank()) {
            _ui.update { it.copy(error = "先在「和风天气」里填写 API Host 和 Key，才能搜索城市") }
            return@launch
        }
        runCatching { api.searchCity(f.apiHost, f.apiKey, q) }
            .onSuccess { c -> _ui.update { it.copy(cities = c, error = null) } }
            .onFailure { e -> _ui.update { it.copy(cities = emptyList(), error = e.message) } }
    }

    fun clearCities() = _ui.update { it.copy(cities = emptyList()) }
}

@Composable
fun SettingsScreen(vm: SettingsViewModel) {
    val ui by vm.ui.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { vm.load() }
    val f = ui.form
    var citySearch by rememberSaveable { mutableStateOf(false) }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        ui.error?.let { ErrorBanner(it) }
        if (f == null) return@Column

        InkCard(title = "城市") {
            ListItem(
                headlineContent = { Text(f.cityName.ifEmpty { "未设置" }, fontWeight = FontWeight.Bold) },
                supportingContent = { Text("和风位置 ID ${f.location}") },
                trailingContent = { TextButton(onClick = { citySearch = true }) { Text("更换") } },
            )
        }

        DayCard("工作日", f.workday) { vm.edit(f.copy(workday = it)) }
        DayCard("休息日（周末与节假日）", f.offday) { vm.edit(f.copy(offday = it)) }

        InkCard(title = "免打扰") {
            Text("这段时间内即使有天气预警也不切换画面。", style = MaterialTheme.typography.bodySmall)
            Row(verticalAlignment = Alignment.CenterVertically) {
                TimeField("从", f.alertTo) { vm.edit(f.copy(alertTo = it)) }
                Spacer(Modifier.width(12.dp))
                TimeField("到次日", f.alertFrom) { vm.edit(f.copy(alertFrom = it)) }
            }
        }

        InkCard(title = "提示阈值") {
            LabeledSlider("温差 ≥ ${f.tempChange}°C 提示增减衣物", f.tempChange.toFloat(), 1f..15f, 13) { vm.edit(f.copy(tempChange = it.roundToInt())) }
            LabeledSlider("降水概率 ≥ ${f.rainPct}% 提示带伞", f.rainPct.toFloat(), 10f..90f, 15) { vm.edit(f.copy(rainPct = (it / 5).roundToInt() * 5)) }
        }

        InkCard(title = "照片") {
            LabeledSlider("饱和度 %.2f".format(f.saturation), f.saturation.toFloat(), 0.8f..2f, 23) { vm.edit(f.copy(saturation = it.round2())) }
            LabeledSlider("对比度 %.2f".format(f.contrast), f.contrast.toFloat(), 0.8f..1.6f, 15) { vm.edit(f.copy(contrast = it.round2())) }
            LabeledSlider("亮度 %.2f".format(f.brightness), f.brightness.toFloat(), 0.8f..1.4f, 11) { vm.edit(f.copy(brightness = it.round2())) }
            LabeledSlider("收藏照片每轮出现 ${f.favoriteWeight} 次", f.favoriteWeight.toFloat(), 1f..5f, 3) { vm.edit(f.copy(favoriteWeight = it.roundToInt())) }
        }

        InkCard(title = "和风天气") {
            OutlinedTextField(f.apiHost, { vm.edit(f.copy(apiHost = it.trim())) }, label = { Text("API Host") }, singleLine = true, modifier = Modifier.fillMaxWidth())
            OutlinedTextField(f.apiKey, { vm.edit(f.copy(apiKey = it.trim())) }, label = { Text("API Key") }, singleLine = true, modifier = Modifier.fillMaxWidth())
        }

        ui.note?.let { Text(it, color = MaterialTheme.colorScheme.tertiary) }
        Button(onClick = vm::save, enabled = !ui.saving && f != ui.saved, modifier = Modifier.fillMaxWidth()) {
            Text(if (ui.saving) "保存中…" else "保存到相框")
        }
    }

    if (citySearch) {
        CityDialog(
            cities = ui.cities,
            onSearch = vm::searchCity,
            onPick = { c ->
                f?.let { vm.edit(it.copy(cityName = c.name, location = c.id)) }
                vm.clearCities()
                citySearch = false
            },
            onDismiss = { vm.clearCities(); citySearch = false },
        )
    }
}

private fun Float.round2() = (this * 100).roundToInt() / 100.0

@Composable
private fun LabeledSlider(label: String, value: Float, range: ClosedFloatingPointRange<Float>, steps: Int, onChange: (Float) -> Unit) {
    Column {
        Text(label, style = MaterialTheme.typography.bodyMedium)
        Slider(value = value.coerceIn(range), onValueChange = onChange, valueRange = range, steps = steps)
    }
}

/** Morning dashboard, photo times (with an every-N-minutes generator), evening dashboard. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun DayCard(title: String, day: DayPlan, onChange: (DayPlan) -> Unit) {
    var generator by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf(false) }
    InkCard(title = title) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TimeField("早间仪表盘", day.morning) { onChange(day.copy(morning = it)) }
            Spacer(Modifier.width(12.dp))
            TimeField("晚间仪表盘", day.evening) { onChange(day.copy(evening = it)) }
        }
        val every = Times.interval(day.photos)
        Text(
            "照片轮播" + when {
                day.photos.isEmpty() -> "：不轮播"
                every != null -> "：${day.photos.first()}–${day.photos.last()} 每 ${every} 分钟"
                else -> "：${day.photos.size} 次"
            },
            style = MaterialTheme.typography.labelLarge,
        )
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            day.photos.sortedBy { Times.parse(it) ?: 0 }.forEach { t ->
                InputChip(
                    selected = false,
                    onClick = { onChange(day.copy(photos = day.photos - t)) },
                    label = { Text(t) },
                    trailingIcon = { Icon(Icons.Default.Close, "移除 $t") },
                )
            }
            AssistChip(onClick = { adding = true }, label = { Text("添加") }, leadingIcon = { Icon(Icons.Default.Add, null) })
        }
        OutlinedButton(onClick = { generator = true }) { Text("按间隔生成…") }
    }
    if (adding) TimeDialog("12:00", onDismiss = { adding = false }) { onChange(day.copy(photos = (day.photos + it).distinct())); adding = false }
    if (generator) IntervalDialog(day.photos, onDismiss = { generator = false }) { onChange(day.copy(photos = it)); generator = false }
}

@Composable
private fun IntervalDialog(current: List<String>, onDismiss: () -> Unit, onApply: (List<String>) -> Unit) {
    var from by remember { mutableStateOf(current.firstOrNull() ?: "18:00") }
    var to by remember { mutableStateOf(current.lastOrNull() ?: "21:00") }
    var every by remember { mutableStateOf((Times.interval(current) ?: 60).toString()) }
    val series = Times.series(from, to, every.toIntOrNull() ?: 0)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("轮播间隔") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row {
                    TimeField("从", from) { from = it }
                    Spacer(Modifier.width(12.dp))
                    TimeField("到", to) { to = it }
                }
                OutlinedTextField(every, { every = it.filter(Char::isDigit).take(4) }, label = { Text("每隔（分钟）") }, singleLine = true)
                Text(if (series.isEmpty()) "时间范围无效" else series.joinToString("  "), style = MaterialTheme.typography.bodySmall)
            }
        },
        confirmButton = { TextButton(onClick = { onApply(series) }, enabled = series.isNotEmpty() && series.size <= 48) { Text("应用") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("取消") } },
    )
}

@Composable
private fun TimeField(label: String, value: String, onChange: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Column(Modifier.clickable { open = true }.padding(vertical = 4.dp)) {
        Text(label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Black)
    }
    if (open) TimeDialog(value, onDismiss = { open = false }) { onChange(it); open = false }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TimeDialog(initial: String, onDismiss: () -> Unit, onPick: (String) -> Unit) {
    val m = Times.parse(initial) ?: 12 * 60
    val state = rememberTimePickerState(initialHour = m / 60, initialMinute = m % 60, is24Hour = true)
    AlertDialog(
        onDismissRequest = onDismiss,
        text = { TimePicker(state) },
        confirmButton = { TextButton(onClick = { onPick(Times.format(state.hour * 60 + state.minute)) }) { Text("确定") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("取消") } },
    )
}

@Composable
private fun CityDialog(cities: List<City>, onSearch: (String) -> Unit, onPick: (City) -> Unit, onDismiss: () -> Unit) {
    var q by rememberSaveable { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("选择城市") },
        text = {
            Column {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(q, { q = it }, singleLine = true, label = { Text("城市名，如 深圳") }, modifier = Modifier.weight(1f))
                    TextButton(onClick = { onSearch(q.trim()) }, enabled = q.isNotBlank()) { Text("搜索") }
                }
                LazyColumn {
                    items(cities, key = { it.id }) { c ->
                        ListItem(headlineContent = { Text(c.name) }, supportingContent = { Text(c.region) }, modifier = Modifier.clickable { onPick(c) })
                        HorizontalDivider()
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("取消") } },
    )
}
