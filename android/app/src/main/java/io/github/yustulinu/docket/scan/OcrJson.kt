package io.github.yustulinu.docket.scan

import org.json.JSONObject
import uniffi.docket_core.Line
import uniffi.docket_core.PageInput

/** The native scan's JSON (as docket-ocr writes a page) and its JPEG, as the core's PageInput. */
fun pageInput(json: String, jpeg: ByteArray): PageInput {
    val o = JSONObject(json)
    val lines = o.getJSONArray("lines")
    return PageInput(
        image = jpeg,
        text = o.getString("text"),
        lines = (0 until lines.length()).map { i ->
            val l = lines.getJSONObject(i)
            Line(
                text = l.getString("text"),
                x = l.getInt("x").toUInt(),
                y = l.getInt("y").toUInt(),
                w = l.getInt("w").toUInt(),
                h = l.getInt("h").toUInt(),
                confidence = l.getDouble("confidence").toFloat(),
            )
        },
        width = o.getInt("width").toUInt(),
        height = o.getInt("height").toUInt(),
        confidence = o.getDouble("confidence").toFloat(),
    )
}
