package com.inkframe.app.data

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

/** `HH:MM` helpers, matching `common::time::parse_hhmm`. */
object Times {
    fun parse(s: String): Int? {
        val m = Regex("""^(\d{1,2}):(\d{2})$""").matchEntire(s.trim()) ?: return null
        val (h, min) = m.destructured.toList().map(String::toInt)
        return if (h in 0..23 && min in 0..59) h * 60 + min else null
    }

    fun format(minutes: Int): String = "%02d:%02d".format(minutes / 60 % 24, minutes % 60)

    /** Photo times from [start] to [end] inclusive, every [everyMinutes]. */
    fun series(start: String, end: String, everyMinutes: Int): List<String> {
        val a = parse(start) ?: return emptyList()
        val b = parse(end) ?: return emptyList()
        if (everyMinutes <= 0 || b < a) return emptyList()
        return (a..b step everyMinutes).map(::format)
    }

    /** The interval if [times] are evenly spaced (and at least two), else null. */
    fun interval(times: List<String>): Int? {
        val m = times.map { parse(it) ?: return null }
        if (m.size < 2) return null
        val d = m[1] - m[0]
        return d.takeIf { it > 0 && m.zipWithNext().all { (x, y) -> y - x == d } }
    }
}

data class DayPlan(val morning: String, val evening: String, val photos: List<String>)

/**
 * The settings the app edits. Read from and written into the full config
 * JSON, so fields this app does not know are preserved.
 */
data class SettingsForm(
    val provider: String,
    val apiHost: String,
    val apiKey: String,
    val cityName: String,
    val location: String,
    val workday: DayPlan,
    val offday: DayPlan,
    val tempChange: Int,
    val rainPct: Int,
    /** Weather warnings may interrupt only inside [alertFrom]–[alertTo]; outside is quiet time. */
    val alertFrom: String,
    val alertTo: String,
    val saturation: Double,
    val contrast: Double,
    val brightness: Double,
    val favoriteWeight: Int,
) {
    /** Problems that would make the device reject the config, in Chinese for the UI. */
    fun errors(): List<String> = buildList {
        for ((label, d) in listOf("工作日" to workday, "休息日" to offday)) {
            if (Times.parse(d.morning) == null) add("$label 早间时间格式应为 HH:MM")
            if (Times.parse(d.evening) == null) add("$label 晚间时间格式应为 HH:MM")
            d.photos.filter { Times.parse(it) == null }.forEach { add("$label 照片时间「$it」无效") }
        }
        if (Times.parse(alertFrom) == null || Times.parse(alertTo) == null) add("免打扰时段格式应为 HH:MM")
        if (tempChange !in 1..20) add("温差阈值应在 1–20°C")
        if (rainPct !in 1..100) add("降水概率阈值应在 1–100%")
    }

    fun writeTo(cfg: JsonObject): JsonObject = cfg
        .set("weather", "city_name", JsonPrimitive(cityName))
        .set("weather", "location", JsonPrimitive(location))
        .set("weather", "api_host", JsonPrimitive(apiHost))
        .set("weather", "api_key", JsonPrimitive(apiKey))
        .set("schedule", "workday", workday.toJson())
        .set("schedule", "offday", offday.toJson())
        .set("schedule", "alert_window", JsonArray(listOf(JsonPrimitive(alertFrom), JsonPrimitive(alertTo))))
        .set("thresholds", "temp_change_c", JsonPrimitive(tempChange))
        .set("thresholds", "rain_probability_pct", JsonPrimitive(rainPct))
        .set("photo", "saturation", JsonPrimitive(saturation))
        .set("photo", "contrast", JsonPrimitive(contrast))
        .set("photo", "brightness", JsonPrimitive(brightness))
        .set("photo", "favorite_weight", JsonPrimitive(favoriteWeight))

    companion object {
        fun read(cfg: JsonObject): SettingsForm {
            fun obj(k: String) = cfg[k] as? JsonObject ?: JsonObject(emptyMap())
            fun JsonObject.s(k: String, d: String = "") = (this[k] as? JsonPrimitive)?.contentOrNull ?: d
            fun JsonObject.i(k: String, d: Int) = (this[k] as? JsonPrimitive)?.intOrNull ?: d
            fun JsonObject.f(k: String, d: Double) = (this[k] as? JsonPrimitive)?.doubleOrNull ?: d
            fun day(o: JsonObject?) = DayPlan(
                morning = o?.s("morning", "07:00") ?: "07:00",
                evening = o?.s("evening", "22:30") ?: "22:30",
                photos = (o?.get("photos") as? JsonArray)?.mapNotNull { it.jsonPrimitive.contentOrNull }.orEmpty(),
            )
            val w = obj("weather")
            val s = obj("schedule")
            val t = obj("thresholds")
            val p = obj("photo")
            val window = (s["alert_window"] as? JsonArray)?.mapNotNull { it.jsonPrimitive.contentOrNull }.orEmpty()
            return SettingsForm(
                provider = w.s("provider", "qweather"),
                apiHost = w.s("api_host"),
                apiKey = w.s("api_key"),
                cityName = w.s("city_name"),
                location = w.s("location"),
                workday = day(s["workday"] as? JsonObject),
                offday = day(s["offday"] as? JsonObject),
                tempChange = t.i("temp_change_c", 5),
                rainPct = t.i("rain_probability_pct", 40),
                alertFrom = window.getOrNull(0) ?: "07:00",
                alertTo = window.getOrNull(1) ?: "22:30",
                saturation = p.f("saturation", 1.4),
                contrast = p.f("contrast", 1.15),
                brightness = p.f("brightness", 1.05),
                favoriteWeight = p.i("favorite_weight", 3),
            )
        }
    }
}

private fun DayPlan.toJson() = JsonObject(
    mapOf(
        "morning" to JsonPrimitive(morning),
        "photos" to JsonArray(photos.sortedBy { Times.parse(it) ?: 0 }.distinct().map(::JsonPrimitive)),
        "evening" to JsonPrimitive(evening),
    ),
)

/** Copy of this object with `section.key` replaced. */
private fun JsonObject.set(section: String, key: String, value: JsonElement): JsonObject {
    val inner = (this[section] as? JsonObject).orEmpty()
    return JsonObject(this + (section to JsonObject(inner + (key to value))))
}

