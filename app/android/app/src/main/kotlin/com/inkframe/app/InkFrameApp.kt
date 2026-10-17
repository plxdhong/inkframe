package com.inkframe.app

import android.app.Application
import com.inkframe.app.data.Device
import com.inkframe.app.data.DeviceStore
import com.inkframe.app.data.InkFrameApi
import com.inkframe.app.data.newHttpClient

/** Manual dependency container; small enough not to need a DI framework. */
class InkFrameApp : Application() {
    val http by lazy { newHttpClient() }
    val devices by lazy { DeviceStore(this) }

    fun api(device: Device) = InkFrameApi(http, device)
}
