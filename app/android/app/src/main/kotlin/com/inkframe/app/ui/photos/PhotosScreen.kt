package com.inkframe.app.ui.photos

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.Tv
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import coil3.compose.AsyncImage
import com.inkframe.app.data.InkFrameApi
import com.inkframe.app.data.PhotoMeta
import com.inkframe.app.ui.ErrorBanner
import com.inkframe.app.ui.theme.Ink
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class PhotosUi(val photos: List<PhotoMeta> = emptyList(), val loading: Boolean = true, val error: String? = null, val note: String? = null)

class PhotosViewModel(val api: InkFrameApi) : ViewModel() {
    private val _ui = MutableStateFlow(PhotosUi())
    val ui = _ui.asStateFlow()

    fun load() = viewModelScope.launch {
        runCatching { api.photos() }
            .onSuccess { p -> _ui.update { it.copy(photos = p.sortedByDescending(PhotoMeta::added), loading = false, error = null) } }
            .onFailure { e -> _ui.update { it.copy(loading = false, error = e.message) } }
    }

    private fun act(note: String? = null, block: suspend () -> Unit) = viewModelScope.launch {
        runCatching { block() }
            .onSuccess { _ui.update { it.copy(note = note) } }
            .onFailure { e -> _ui.update { it.copy(error = e.message) } }
        load()
    }

    fun favorite(p: PhotoMeta) = act { api.updatePhoto(p.id, favorite = !p.favorite) }
    fun pause(p: PhotoMeta) = act { api.updatePhoto(p.id, paused = !p.paused) }
    fun delete(p: PhotoMeta) = act { api.deletePhoto(p.id) }
    fun show(p: PhotoMeta) = act("已发送到相框，约 25 秒后显示") { api.showPhoto(p.id) }
}

@Composable
fun PhotosScreen(vm: PhotosViewModel) {
    val ui by vm.ui.collectAsStateWithLifecycle()
    var confirmDelete by remember { mutableStateOf<PhotoMeta?>(null) }
    LaunchedEffect(vm) { vm.load() }

    Column(Modifier.fillMaxSize()) {
        ui.error?.let { ErrorBanner(it, Modifier.padding(16.dp)) }
        ui.note?.let { Text(it, Modifier.padding(horizontal = 16.dp, vertical = 8.dp), style = MaterialTheme.typography.bodySmall) }
        if (!ui.loading && ui.photos.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("还没有照片，点右下角添加。", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        LazyVerticalGrid(
            columns = GridCells.Adaptive(160.dp),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(start = 12.dp, end = 12.dp, top = 12.dp, bottom = 96.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            items(ui.photos, key = { it.id }) { p ->
                PhotoTile(
                    url = vm.api.photoUrl(p.id),
                    photo = p,
                    onFavorite = { vm.favorite(p) },
                    onPause = { vm.pause(p) },
                    onShow = { vm.show(p) },
                    onDelete = { confirmDelete = p },
                )
            }
        }
    }
    confirmDelete?.let { p ->
        AlertDialog(
            onDismissRequest = { confirmDelete = null },
            title = { Text("删除这张照片？") },
            text = { Text("相框上的原图和处理结果都会删除。") },
            confirmButton = { TextButton(onClick = { vm.delete(p); confirmDelete = null }) { Text("删除", color = MaterialTheme.colorScheme.error) } },
            dismissButton = { TextButton(onClick = { confirmDelete = null }) { Text("取消") } },
        )
    }
}

@Composable
private fun PhotoTile(url: String, photo: PhotoMeta, onFavorite: () -> Unit, onPause: () -> Unit, onShow: () -> Unit, onDelete: () -> Unit) {
    Column(Modifier.border(2.dp, MaterialTheme.colorScheme.outline).background(MaterialTheme.colorScheme.surfaceVariant)) {
        Box {
            AsyncImage(
                model = url,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxWidth().aspectRatio(5f / 3f).alpha(if (photo.paused) 0.4f else 1f),
            )
            if (photo.paused) Text("已暂停", Modifier.align(Alignment.Center).background(Ink.Black).padding(horizontal = 6.dp, vertical = 2.dp), color = Ink.Paper)
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            IconButton(onClick = onFavorite) {
                Icon(if (photo.favorite) Icons.Default.Favorite else Icons.Default.FavoriteBorder, if (photo.favorite) "取消收藏" else "收藏", tint = if (photo.favorite) Ink.Red else MaterialTheme.colorScheme.onSurface)
            }
            IconButton(onClick = onPause) { Icon(if (photo.paused) Icons.Default.PlayArrow else Icons.Default.Pause, if (photo.paused) "恢复轮播" else "暂停轮播") }
            IconButton(onClick = onShow) { Icon(Icons.Outlined.Tv, "立即显示") }
            IconButton(onClick = onDelete) { Icon(Icons.Default.Delete, "删除") }
        }
    }
}
