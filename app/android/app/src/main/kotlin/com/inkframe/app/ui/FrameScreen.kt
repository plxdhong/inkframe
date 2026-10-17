package com.inkframe.app.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AddPhotoAlternate
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.PhotoLibrary
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.SwapHoriz
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.lifecycle.viewmodel.compose.viewModel
import com.inkframe.app.data.Device
import com.inkframe.app.data.InkFrameApi
import com.inkframe.app.ui.home.HomeScreen
import com.inkframe.app.ui.home.HomeViewModel
import com.inkframe.app.ui.photos.PhotosScreen
import com.inkframe.app.ui.photos.PhotosViewModel
import com.inkframe.app.ui.settings.SettingsScreen
import com.inkframe.app.ui.settings.SettingsViewModel

/** One frame: status, photos and settings tabs. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FrameScreen(device: Device, api: InkFrameApi, onSwitch: () -> Unit, onAddPhoto: (String) -> Unit) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    val home = viewModel(key = "home-${device.id}") { HomeViewModel(api) }
    val photos = viewModel(key = "photos-${device.id}") { PhotosViewModel(api) }
    val settings = viewModel(key = "settings-${device.id}") { SettingsViewModel(api) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        uri?.let { onAddPhoto(it.toString()) }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(device.name, fontWeight = FontWeight.Black) },
                actions = { IconButton(onClick = onSwitch) { Icon(Icons.Default.SwapHoriz, "切换相框") } },
            )
        },
        bottomBar = {
            NavigationBar {
                listOf(Triple("相框", Icons.Default.Home, 0), Triple("照片", Icons.Default.PhotoLibrary, 1), Triple("设置", Icons.Default.Settings, 2))
                    .forEach { (label, icon, i) ->
                        NavigationBarItem(selected = tab == i, onClick = { tab = i }, icon = { Icon(icon, null) }, label = { Text(label) })
                    }
            }
        },
        floatingActionButton = {
            if (tab == 1) {
                ExtendedFloatingActionButton(
                    onClick = { pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                    icon = { Icon(Icons.Default.AddPhotoAlternate, null) },
                    text = { Text("添加照片") },
                )
            }
        },
    ) { pad ->
        val m = Modifier.padding(pad)
        androidx.compose.foundation.layout.Box(m) {
            when (tab) {
                0 -> HomeScreen(home)
                1 -> PhotosScreen(photos)
                else -> SettingsScreen(settings)
            }
        }
    }
}
