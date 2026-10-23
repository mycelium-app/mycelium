package mycelium.state

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.jsonObject
import kotlin.math.roundToInt

const val GeneratedUiLanguage = "mycelium-ui"

private const val DeepestNesting = 8

data class UiBar(val label: String, val value: Double, val display: String)

sealed interface UiAction {
    data class SendMessage(val text: String) : UiAction

    data class SideChat(val text: String) : UiAction

    data class OpenFile(val path: String) : UiAction
}

sealed interface UiNode {
    data class Column(val children: List<UiNode>) : UiNode

    data class Row(val children: List<UiNode>) : UiNode

    data class Heading(val text: String) : UiNode

    data class Text(val text: String) : UiNode

    data class Table(val columns: List<String>, val rows: List<List<String>>) : UiNode

    data class BarChart(val title: String?, val bars: List<UiBar>) : UiNode

    data class Progress(val label: String, val share: Float, val display: String) : UiNode

    data class Button(val label: String, val action: UiAction) : UiNode
}

fun parseGeneratedUi(source: String): UiNode? {
    val root = runCatching { Json.parseToJsonElement(source) }.getOrNull() ?: return null

    return node(root, 0)
}

fun barChartDescription(chart: UiNode.BarChart): String =
    (listOfNotNull(chart.title) + chart.bars.map { "${it.label}: ${it.display}" }).joinToString(". ")

private fun JsonObject.text(field: String): String? = (this[field] as? JsonPrimitive)?.takeIf { it.isString }?.content

private fun JsonObject.strings(field: String): List<String>? = (this[field] as? JsonArray)?.map { (it as? JsonPrimitive)?.takeIf { cell -> cell.isString }?.content ?: return null }

private fun node(element: JsonElement, depth: Int): UiNode? {
    val fields = (element as? JsonObject) ?: return null

    if (depth > DeepestNesting) {
        return null
    }

    return when (fields.text("type")) {
        "column" -> children(fields, depth)?.let { UiNode.Column(it) }
        "row" -> children(fields, depth)?.let { UiNode.Row(it) }
        "heading" -> fields.text("text")?.let { UiNode.Heading(it) }
        "text" -> fields.text("text")?.let { UiNode.Text(it) }
        "table" -> table(fields)
        "bar_chart" -> chart(fields)
        "progress" -> progress(fields)
        "button" -> button(fields)
        else -> null
    }
}

private fun children(fields: JsonObject, depth: Int): List<UiNode>? {
    val items = fields["children"] as? JsonArray ?: return null

    return items.map { node(it, depth + 1) ?: return null }
}

private fun table(fields: JsonObject): UiNode.Table? {
    val columns = fields.strings("columns")?.takeIf { it.isNotEmpty() } ?: return null
    val rows = (fields["rows"] as? JsonArray)?.map { row -> (row as? JsonArray)?.let { cells -> List(columns.size) { index -> (cells.getOrNull(index) as? JsonPrimitive)?.content.orEmpty() } } ?: return null } ?: return null

    return UiNode.Table(columns, rows)
}

private fun chart(fields: JsonObject): UiNode.BarChart? {
    val bars = (fields["bars"] as? JsonArray)?.takeIf { it.isNotEmpty() }?.map { entry ->
        val bar = entry as? JsonObject ?: return null
        val label = bar.text("label") ?: return null
        val value = (bar["value"] as? JsonPrimitive)?.takeIf { !it.isString }?.doubleOrNull ?: return null

        UiBar(label, value, bar.text("display") ?: value.toString().removeSuffix(".0"))
    } ?: return null

    return UiNode.BarChart(fields.text("title"), bars)
}

private fun progress(fields: JsonObject): UiNode.Progress? {
    val label = fields.text("label") ?: return null
    val value = (fields["value"] as? JsonPrimitive)?.takeIf { !it.isString }?.doubleOrNull ?: return null
    val max = (fields["max"] as? JsonPrimitive)?.takeIf { !it.isString }?.doubleOrNull ?: 100.0

    if (max <= 0) {
        return null
    }

    val share = (value / max).coerceIn(0.0, 1.0)

    return UiNode.Progress(label, share.toFloat(), fields.text("display") ?: "${(share * 100).roundToInt()}%")
}

private fun button(fields: JsonObject): UiNode.Button? {
    val label = fields.text("label") ?: return null
    val action = (fields["action"] as? JsonObject)?.jsonObject ?: return null

    val parsed = when (action.text("type")) {
        "send_message" -> action.text("text")?.let { UiAction.SendMessage(it) }
        "side_chat" -> action.text("text")?.let { UiAction.SideChat(it) }
        "open_file" -> action.text("path")?.let { UiAction.OpenFile(it) }
        else -> null
    }

    return parsed?.let { UiNode.Button(label, it) }
}
