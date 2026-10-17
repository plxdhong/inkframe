package com.inkframe.app

import com.inkframe.app.data.CropState
import org.junit.Assert.assertArrayEquals
import org.junit.Test

class CropTest {
    private fun rect(c: CropState, w: Int, h: Int) = c.clamp(w, h, 500f, 300f).sourceRect(w, h, 500f, 300f)

    @Test
    fun defaultIsCentreCover() {
        // 4000x3000 into 5:3: full width, centred 2400 rows.
        assertArrayEquals(floatArrayOf(0f, 300f, 4000f, 2700f), rect(CropState(), 4000, 3000), 0.5f)
        // Portrait: full height is too tall, so full width and centred.
        assertArrayEquals(floatArrayOf(0f, 1800f, 3000f, 3600f), rect(CropState(), 3000, 5400), 0.5f)
    }

    @Test
    fun panIsClampedToTheImage() {
        // Dragging far right shows the left edge, never beyond it.
        assertArrayEquals(floatArrayOf(0f, 300f, 4000f, 2700f), rect(CropState(dx = 9999f), 4000, 3000), 0.5f)
        assertArrayEquals(floatArrayOf(0f, 0f, 4000f, 2400f), rect(CropState(dy = 9999f), 4000, 3000), 0.5f)
    }

    @Test
    fun zoomShrinksTheSource() {
        assertArrayEquals(floatArrayOf(1000f, 900f, 3000f, 2100f), rect(CropState(zoom = 2f), 4000, 3000), 0.5f)
        // Zoom below 1 would leave gaps, so it is clamped.
        assertArrayEquals(floatArrayOf(0f, 300f, 4000f, 2700f), rect(CropState(zoom = 0.5f), 4000, 3000), 0.5f)
    }
}
