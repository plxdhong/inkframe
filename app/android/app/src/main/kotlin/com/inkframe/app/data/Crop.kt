package com.inkframe.app.data

import kotlin.math.max
import kotlin.math.min

/**
 * The 5:3 crop chosen by pan/zoom. The image is drawn "cover" into a
 * [viewW]×[viewH] viewport, then scaled by [zoom] (≥ 1) about the centre and
 * shifted by ([dx], [dy]) viewport pixels.
 */
data class CropState(val zoom: Float = 1f, val dx: Float = 0f, val dy: Float = 0f) {
    /** Keeps the viewport fully covered by the image. */
    fun clamp(imgW: Int, imgH: Int, viewW: Float, viewH: Float): CropState {
        val z = zoom.coerceIn(1f, 8f)
        val s = coverScale(imgW, imgH, viewW, viewH) * z
        val maxX = max(0f, (imgW * s - viewW) / 2f)
        val maxY = max(0f, (imgH * s - viewH) / 2f)
        return CropState(z, dx.coerceIn(-maxX, maxX), dy.coerceIn(-maxY, maxY))
    }

    /** Source rectangle in image pixels: left, top, right, bottom. */
    fun sourceRect(imgW: Int, imgH: Int, viewW: Float, viewH: Float): FloatArray {
        val s = coverScale(imgW, imgH, viewW, viewH) * zoom
        val w = viewW / s
        val h = viewH / s
        val left = imgW / 2f - (viewW / 2f + dx) / s
        val top = imgH / 2f - (viewH / 2f + dy) / s
        val l = left.coerceIn(0f, max(0f, imgW - w))
        val t = top.coerceIn(0f, max(0f, imgH - h))
        return floatArrayOf(l, t, min(imgW.toFloat(), l + w), min(imgH.toFloat(), t + h))
    }

    companion object {
        const val OUT_W = 800
        const val OUT_H = 480

        fun coverScale(imgW: Int, imgH: Int, viewW: Float, viewH: Float) = max(viewW / imgW, viewH / imgH)
    }
}
