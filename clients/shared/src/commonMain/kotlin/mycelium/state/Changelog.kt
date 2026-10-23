package mycelium.state

const val LastLaunchedVersionPreference = "last-launched-version"

const val WhatsNewSettingsCount = 5

data class Release(val title: String, val version: String?, val notes: List<String>)

fun versionParts(version: String): List<Int>? {
    val parts = version.trim().removePrefix("v").split(".").map { it.toIntOrNull() }

    return if (parts.any { it == null }) null else parts.filterNotNull()
}

fun compareVersions(left: List<Int>, right: List<Int>): Int {
    for (index in 0 until maxOf(left.size, right.size)) {
        val difference = left.getOrElse(index) { 0 }.compareTo(right.getOrElse(index) { 0 })

        if (difference != 0) {
            return difference
        }
    }

    return 0
}

fun parseChangelog(text: String): List<Release> {
    val releases = mutableListOf<Release>()
    var title: String? = null
    var notes = mutableListOf<String>()

    fun finish() {
        val finished = title ?: return
        val version = versionParts(finished)?.joinToString(".")

        releases += Release(finished, version, notes)
    }

    for (line in text.lines()) {
        if (line.startsWith("## ")) {
            finish()
            title = line.removePrefix("## ").trim()
            notes = mutableListOf()
        } else if (title != null && line.startsWith("- ")) {
            notes += line.removePrefix("- ").trim()
        }
    }

    finish()

    return releases
}

fun releasesSince(releases: List<Release>, lastLaunched: String?, running: String): List<Release> {
    val last = lastLaunched?.let(::versionParts) ?: return emptyList()
    val current = versionParts(running) ?: return emptyList()

    return releases.filter { release ->
        val parts = release.version?.let(::versionParts)

        parts != null && compareVersions(parts, last) > 0 && compareVersions(parts, current) <= 0
    }
}
