package com.inkframe.app

import com.inkframe.app.data.AppJson
import com.inkframe.app.data.SettingsForm
import com.inkframe.app.data.Times
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SettingsFormTest {
    private val cfg = AppJson.parseToJsonElement(
        """
        {"version":1,"timezone":"Asia/Shanghai",
         "weather":{"provider":"qweather","api_host":"h","api_key":"k","location":"101280601","city_name":"深圳","poll_minutes":60},
         "schedule":{"workday":{"morning":"07:00","photos":["18:00","19:00","20:00","21:00"],"evening":"22:30"},
                     "offday":{"morning":"08:00","photos":["10:00","12:00"],"evening":"22:30"},
                     "alert_window":["07:00","22:30"]},
         "thresholds":{"temp_change_c":5,"rain_probability_pct":40},
         "photo":{"saturation":1.4,"contrast":1.15,"brightness":1.05,"favorite_weight":3,"max_dimension":2400},
         "future_section":{"x":1}}
        """,
    ) as JsonObject

    @Test
    fun roundTripKeepsUnknownFields() {
        val form = SettingsForm.read(cfg)
        assertEquals("深圳", form.cityName)
        assertEquals(listOf("18:00", "19:00", "20:00", "21:00"), form.workday.photos)
        val out = form.copy(cityName = "北京", location = "101010100", tempChange = 4).writeTo(cfg)
        assertEquals("北京", out["weather"]!!.jsonObject["city_name"]!!.jsonPrimitive.content)
        assertEquals("60", out["weather"]!!.jsonObject["poll_minutes"]!!.jsonPrimitive.content)
        assertEquals("2400", out["photo"]!!.jsonObject["max_dimension"]!!.jsonPrimitive.content)
        assertTrue("future_section" in out)
        assertEquals(form.copy(cityName = "北京", location = "101010100", tempChange = 4), SettingsForm.read(out))
    }

    @Test
    fun photoTimesAreSortedAndDeduplicated() {
        val form = SettingsForm.read(cfg)
        val out = form.copy(offday = form.offday.copy(photos = listOf("14:00", "10:00", "14:00"))).writeTo(cfg)
        assertEquals(listOf("10:00", "14:00"), SettingsForm.read(out).offday.photos)
    }

    @Test
    fun validation() {
        val form = SettingsForm.read(cfg)
        assertTrue(form.errors().isEmpty())
        assertEquals(1, form.copy(alertTo = "25:00").errors().size)
        assertEquals(1, form.copy(workday = form.workday.copy(photos = listOf("7pm"))).errors().size)
    }

    @Test
    fun timeSeries() {
        assertEquals(listOf("10:00", "12:00", "14:00"), Times.series("10:00", "14:00", 120))
        assertEquals(listOf("18:00", "18:45", "19:30"), Times.series("18:00", "19:59", 45))
        assertEquals(emptyList<String>(), Times.series("20:00", "10:00", 60))
        assertEquals(60, Times.interval(listOf("18:00", "19:00", "20:00")))
        assertNull(Times.interval(listOf("18:00", "19:00", "21:00")))
        assertNull(Times.parse("24:00"))
    }
}
