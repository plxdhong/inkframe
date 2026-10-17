package com.inkframe.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.core.net.toUri
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import com.inkframe.app.data.Device
import com.inkframe.app.ui.FrameScreen
import com.inkframe.app.ui.devices.DevicesScreen
import com.inkframe.app.ui.photos.AddPhotoScreen
import com.inkframe.app.ui.photos.AddPhotoViewModel
import com.inkframe.app.ui.provision.ProvisionScreen
import com.inkframe.app.ui.theme.InkFrameTheme
import kotlinx.coroutines.flow.combine
import kotlinx.serialization.Serializable

@Serializable object DevicesRoute
@Serializable object ProvisionRoute
@Serializable object FrameRoute
@Serializable data class AddPhotoRoute(val uri: String)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            InkFrameTheme {
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) { AppNav() }
            }
        }
    }
}

/** The current frame, or null while loading / when none is chosen. */
private data class Current(val loaded: Boolean, val device: Device?)

@Composable
private fun AppNav() {
    val app = LocalContext.current.applicationContext as InkFrameApp
    val current by remember {
        combine(app.devices.devices, app.devices.currentId) { list, id -> Current(true, list.firstOrNull { it.id == id }) }
    }.collectAsState(Current(false, null))
    if (!current.loaded) {
        Box(Modifier.fillMaxSize())
        return
    }
    val nav = rememberNavController()
    NavHost(nav, startDestination = if (current.device != null) FrameRoute else DevicesRoute) {
        composable<DevicesRoute> {
            DevicesScreen(
                onOpen = { nav.navigate(FrameRoute) { popUpTo(DevicesRoute) } },
                onAdd = { nav.navigate(ProvisionRoute) },
            )
        }
        composable<ProvisionRoute> {
            ProvisionScreen(
                onBack = { nav.popBackStack() },
                onDone = { nav.navigate(FrameRoute) { popUpTo(DevicesRoute) } },
            )
        }
        composable<FrameRoute> {
            val d = current.device
            if (d == null) {
                Box(Modifier.fillMaxSize())
            } else {
                val api = remember(d) { app.api(d) }
                FrameScreen(
                    device = d,
                    api = api,
                    onSwitch = { nav.navigate(DevicesRoute) { popUpTo(0) } },
                    onAddPhoto = { nav.navigate(AddPhotoRoute(it)) },
                )
            }
        }
        composable<AddPhotoRoute> { entry ->
            val d = current.device ?: return@composable
            val uri = entry.toRoute<AddPhotoRoute>().uri.toUri()
            val vm = viewModel { AddPhotoViewModel(app, app.api(d), uri) }
            AddPhotoScreen(vm, onClose = { nav.popBackStack() })
        }
    }
}

