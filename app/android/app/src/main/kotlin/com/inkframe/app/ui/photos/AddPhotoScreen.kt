package com.inkframe.app.ui.photos

import android.app.Application
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.graphics.Rect
import android.net.Uri
import androidx.compose.foundation.Image
import androidx.compose.foundation.gestures.rememberTransformableState
import androidx.compose.foundation.gestures.transformable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.foundation.selection.toggleable
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.dp
import androidx.core.graphics.createBitmap
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import com.inkframe.app.data.CropState
import com.inkframe.app.data.InkFrameApi
import com.inkframe.app.ui.ErrorBanner
import com.inkframe.app.ui.PanelFrame
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream

data class AddPhotoUi(
    val source: Bitmap? = null,
    val crop: CropState = CropState(),
    /** The device's dithering of the current crop, or null if the crop changed since. */
    val dithered: Bitmap? = null,
    val busy: String? = null,
    val error: String? = null,
    val done: Boolean = false,
)

class AddPhotoViewModel(application: Application, private val api: InkFrameApi, private val uri: Uri) : AndroidViewModel(application) {
    private val _ui = MutableStateFlow(AddPhotoUi())
    val ui = _ui.asStateFlow()
    private var viewSize = Size.Zero
    private var croppedPng: ByteArray? = null

    init {
        viewModelScope.launch {
            _ui.update { it.copy(busy = "读取照片…") }
            runCatching { withContext(Dispatchers.IO) { decode(uri) } }
                .onSuccess { b -> _ui.update { it.copy(source = b, busy = null) } }
                .onFailure { e -> _ui.update { it.copy(busy = null, error = "无法读取照片：${e.message}") } }
        }
    }

    /** Decodes downsampled (≤ 2400 px, the device's upload limit) into a software bitmap. */
    private fun decode(uri: Uri): Bitmap {
        val src = ImageDecoder.createSource(getApplication<Application>().contentResolver, uri)
        return ImageDecoder.decodeBitmap(src) { decoder, info, _ ->
            decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
            val longest = maxOf(info.size.width, info.size.height)
            if (longest > MAX_SIDE) {
                val k = MAX_SIDE.toFloat() / longest
                decoder.setTargetSize((info.size.width * k).toInt(), (info.size.height * k).toInt())
            }
        }
    }

    fun onViewSize(s: Size) {
        viewSize = s
    }

    /** Pinch zooms about the fingers ([cx], [cy] in view pixels); drag pans. */
    fun transform(cx: Float, cy: Float, zoomBy: Float, panX: Float, panY: Float) {
        val b = _ui.value.source ?: return
        if (viewSize == Size.Zero) return
        _ui.update {
            val c = it.crop
            // Keep the point under the centroid fixed: d' = p + (d − p)·k, p relative to the centre.
            val px = cx - viewSize.width / 2f
            val py = cy - viewSize.height / 2f
            val k = (c.zoom * zoomBy).coerceIn(1f, 8f) / c.zoom
            val next = CropState(c.zoom * k, px + (c.dx - px) * k + panX, py + (c.dy - py) * k + panY)
            it.copy(crop = next.clamp(b.width, b.height, viewSize.width, viewSize.height), dithered = null)
        }
        croppedPng = null
    }

    /** The current crop at panel resolution, lossless so the device dithers exactly what was previewed. */
    private suspend fun cropPng(): ByteArray = croppedPng ?: withContext(Dispatchers.Default) {
        val b = _ui.value.source ?: error("no image")
        val r = _ui.value.crop.sourceRect(b.width, b.height, viewSize.width, viewSize.height)
        val out = createBitmap(CropState.OUT_W, CropState.OUT_H)
        android.graphics.Canvas(out).drawBitmap(
            b,
            Rect(r[0].toInt(), r[1].toInt(), r[2].toInt(), r[3].toInt()),
            Rect(0, 0, CropState.OUT_W, CropState.OUT_H),
            android.graphics.Paint(android.graphics.Paint.FILTER_BITMAP_FLAG),
        )
        ByteArrayOutputStream().also { out.compress(Bitmap.CompressFormat.PNG, 100, it) }.toByteArray()
    }.also { croppedPng = it }

    fun preview() = viewModelScope.launch {
        _ui.update { it.copy(busy = "相框正在计算六色抖动…", error = null) }
        runCatching {
            val png = api.previewPhoto(cropPng())
            withContext(Dispatchers.Default) { android.graphics.BitmapFactory.decodeByteArray(png, 0, png.size) }
        }
            .onSuccess { bmp -> _ui.update { it.copy(dithered = bmp, busy = null) } }
            .onFailure { e -> _ui.update { it.copy(busy = null, error = "预览失败：${e.message}") } }
    }

    fun push(showNow: Boolean) = viewModelScope.launch {
        _ui.update { it.copy(busy = "上传到相框…", error = null) }
        runCatching {
            val meta = api.uploadPhoto(cropPng())
            if (showNow) api.showPhoto(meta.id)
        }
            .onSuccess { _ui.update { it.copy(busy = null, done = true) } }
            .onFailure { e -> _ui.update { it.copy(busy = null, error = "上传失败：${e.message}") } }
    }

    companion object {
        const val MAX_SIDE = 2400
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddPhotoScreen(vm: AddPhotoViewModel, onClose: () -> Unit) {
    val ui by vm.ui.collectAsStateWithLifecycle()
    var showNow by remember { mutableStateOf(true) }
    LaunchedEffect(ui.done) { if (ui.done) onClose() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("添加照片") },
                navigationIcon = { IconButton(onClick = onClose) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "返回") } },
            )
        },
    ) { pad ->
        Column(
            Modifier.padding(pad).fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            ui.error?.let { ErrorBanner(it) }
            Text("双指缩放、拖动，选择相框上显示的部分（5:3）。", style = MaterialTheme.typography.bodyMedium)
            val transform = rememberTransformableState { centroid, zoom, pan, _ -> vm.transform(centroid.x, centroid.y, zoom, pan.x, pan.y) }
            PanelFrame(Modifier.onSizeChanged { vm.onViewSize(Size(it.width.toFloat(), it.height.toFloat())) }.transformable(transform)) {
                ui.source?.let { b ->
                    val bmp = remember(b) { b.asImageBitmap() }
                    Image(
                        bmp,
                        contentDescription = "裁剪",
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.fillMaxSize().graphicsLayer {
                            scaleX = ui.crop.zoom
                            scaleY = ui.crop.zoom
                            translationX = ui.crop.dx
                            translationY = ui.crop.dy
                            clip = true
                        },
                    )
                }
            }
            Text("在相框上的效果", style = MaterialTheme.typography.titleSmall)
            PanelFrame {
                val dithered = ui.dithered
                if (dithered != null) {
                    Image(dithered.asImageBitmap(), "抖动预览", Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
                } else {
                    OutlinedButton(onClick = vm::preview, enabled = ui.source != null && ui.busy == null) { Text("预览六色效果") }
                }
            }
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth().toggleable(showNow, role = Role.Checkbox) { showNow = it },
            ) {
                Checkbox(showNow, null)
                Text("上传后立即显示")
            }
            Button(onClick = { vm.push(showNow) }, enabled = ui.source != null && ui.busy == null, modifier = Modifier.fillMaxWidth()) {
                Text("推送到相框")
            }
            ui.busy?.let {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    Text(it)
                }
            }
        }
    }
}
