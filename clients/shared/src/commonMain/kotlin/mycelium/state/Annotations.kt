package mycelium.state

data class Annotation(val quote: String, val comment: String)

private const val LabelLimit = 48

fun annotationLabel(annotation: Annotation): String {
    val quote = annotation.quote.replace(Regex("""\s+"""), " ").trim()
    val shown = if (quote.length > LabelLimit) quote.take(LabelLimit).trimEnd() + "..." else quote

    return if (annotation.comment.isBlank()) "\"$shown\"" else "\"$shown\": ${annotation.comment.trim()}"
}

fun messageWithAnnotations(annotations: List<Annotation>, message: String): String {
    val quoted = annotations.map { annotation ->
        val quote = annotation.quote.trim().lines().joinToString("\n") { "> $it" }

        if (annotation.comment.isBlank()) quote else "$quote\n\n${annotation.comment.trim()}"
    }

    return (quoted + message.trim().takeIf { it.isNotEmpty() }).filterNotNull().joinToString("\n\n")
}
