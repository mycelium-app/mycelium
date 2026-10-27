package mycelium.state

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.longOrNull

typealias ResponsePath = List<Any>

enum class KeepState { None, Some, All }

enum class ResponseShape { Table, Tree, Text }

data class ResponseDraft(val base: JsonElement, val dropped: Set<ResponsePath> = emptySet(), val edits: Map<ResponsePath, JsonElement> = emptyMap()) {
    val shape: ResponseShape
        get() = shapeOf(base)

    val rows: List<JsonElement>
        get() = (base as? JsonArray).orEmpty()

    val columns: List<String>
        get() = columnsOf(rows)

    fun valueAt(path: ResponsePath): JsonElement? = path.fold<Any, JsonElement?>(base) { node, step -> child(node, step) }

    fun isDropped(path: ResponsePath): Boolean = (0..path.size).any { path.take(it) in dropped }

    fun isEdited(path: ResponsePath): Boolean = path in edits

    fun displayValue(path: ResponsePath): JsonElement? = edits[path] ?: valueAt(path)

    fun keepState(path: ResponsePath): KeepState {
        if (isDropped(path)) {
            return KeepState.None
        }

        val node = valueAt(path) ?: return KeepState.None
        val leaves = leafPaths(node, path)

        return when {
            leaves.isEmpty() -> KeepState.All
            leaves.all { !isDropped(it) } -> KeepState.All
            leaves.any { !isDropped(it) } -> KeepState.Some
            else -> KeepState.None
        }
    }

    fun toggle(path: ResponsePath): ResponseDraft {
        if (!isDropped(path)) {
            return copy(dropped = dropped + listOf(path))
        }

        val highest = (0..path.size).map { path.take(it) }.first { it in dropped }
        val withoutHighest = dropped.filterNot { it == highest }.toSet()
        var siblings = emptySet<ResponsePath>()

        for (depth in highest.size until path.size) {
            val parent = path.take(depth)
            val towards = path[depth]
            val node = valueAt(parent) ?: continue

            siblings = siblings + childSteps(node).filter { it != towards }.map { step -> parent + listOf(step) }.toSet()
        }

        return copy(dropped = withoutHighest + siblings)
    }

    fun toggleRange(from: Int, to: Int): ResponseDraft {
        val range = minOf(from, to)..maxOf(from, to)
        val keep = isDropped(listOf(to))

        return copy(dropped = if (keep) dropped - range.map { listOf<Any>(it) }.toSet() else dropped + range.map { listOf<Any>(it) })
    }

    fun keepAllRows(): ResponseDraft = copy(dropped = dropped.filterNot { it.size == 1 && it[0] is Int }.toSet())

    fun keepNoRows(): ResponseDraft = copy(dropped = dropped + rows.indices.map { listOf<Any>(it) })

    fun keptRowCount(): Int = rows.indices.count { !isDropped(listOf(it)) }

    fun edit(path: ResponsePath, value: JsonElement): ResponseDraft = copy(edits = if (value == valueAt(path)) edits.filterKeys { it != path } else edits + (path to value))

    fun reset(path: ResponsePath): ResponseDraft = copy(edits = edits.filterKeys { it != path })

    fun select(paths: List<ResponsePath>): ResponseDraft {
        val kept = if (shape == ResponseShape.Table) paths.mapNotNull { (it.firstOrNull() as? Int)?.let { index -> listOf<Any>(index) } }.toSet() else paths.toSet()
        val toDrop = if (shape == ResponseShape.Table) rows.indices.map { listOf<Any>(it) }.filter { it !in kept } else dropUnrelated(base, emptyList(), kept)

        return copy(dropped = toDrop.toSet())
    }

    fun result(): JsonElement = build(base, emptyList())

    private fun dropUnrelated(node: JsonElement, path: ResponsePath, kept: Set<ResponsePath>): List<ResponsePath> {
        if (kept.any { it.take(path.size) == path }) {
            return childSteps(node).flatMap { step -> child(node, step)?.let { dropUnrelated(it, path + listOf(step), kept) }.orEmpty() }
        }

        return if (path.isEmpty()) emptyList() else listOf(path)
    }

    private fun build(node: JsonElement, path: ResponsePath): JsonElement {
        edits[path]?.let { return it }

        return when (node) {
            is JsonObject -> JsonObject(node.entries.filter { !isDropped(path + it.key) }.associate { it.key to build(it.value, path + it.key) })
            is JsonArray -> JsonArray(node.indices.filter { !isDropped(path + it) }.map { build(node[it], path + it) })
            else -> node
        }
    }
}

fun shapeOf(value: JsonElement): ResponseShape = when {
    value is JsonPrimitive && value.isString -> ResponseShape.Text
    value is JsonArray && value.isNotEmpty() && value.all { it is JsonObject } -> ResponseShape.Table
    else -> ResponseShape.Tree
}

fun columnsOf(rows: List<JsonElement>): List<String> = rows.flatMap { (it as? JsonObject)?.keys.orEmpty() }.distinct()

fun pathFrom(steps: List<JsonElement>): ResponsePath = steps.map { step ->
    val primitive = step as? JsonPrimitive

    if (primitive != null && primitive.isString) primitive.content else primitive?.longOrNull?.toInt() ?: step.toString()
}

fun pathLabel(path: ResponsePath): String = path.joinToString(".") { it.toString() }

fun childSteps(node: JsonElement): List<Any> = when (node) {
    is JsonObject -> node.keys.toList()
    is JsonArray -> node.indices.toList()
    else -> emptyList()
}

private fun child(node: JsonElement?, step: Any): JsonElement? = when {
    node is JsonObject && step is String -> node[step]
    node is JsonArray && step is Int -> node.getOrNull(step)
    else -> null
}

private fun leafPaths(node: JsonElement, path: ResponsePath): List<ResponsePath> {
    val steps = childSteps(node)

    if (steps.isEmpty()) {
        return listOf(path)
    }

    return steps.flatMap { step -> child(node, step)?.let { leafPaths(it, path + listOf(step)) }.orEmpty() }
}

fun itemCount(value: JsonElement): String? = when (value) {
    is JsonArray -> if (value.size == 1) "1 item" else "${value.size} items"
    is JsonObject -> if (value.size == 1) "1 field" else "${value.size} fields"
    else -> null
}

fun valueText(value: JsonElement): String = when {
    value is JsonPrimitive && value.isString -> value.content
    else -> value.toString()
}

fun parseEdited(text: String, original: JsonElement): JsonElement? {
    if (original is JsonPrimitive && original.isString) {
        return JsonPrimitive(text)
    }

    val trimmed = text.trim()

    return when {
        trimmed == "null" -> JsonNull
        trimmed == "true" -> JsonPrimitive(true)
        trimmed == "false" -> JsonPrimitive(false)
        trimmed.toLongOrNull() != null -> JsonPrimitive(trimmed.toLong())
        trimmed.toDoubleOrNull() != null -> JsonPrimitive(trimmed.toDouble())
        else -> null
    }
}
