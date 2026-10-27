package mycelium.state

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import mycelium.api.PuzzleResult

data class Puzzle(val id: String, val name: String, val url: String, val host: String, val pathPrefix: String, val storageMarkers: List<String>, val measure: String)

val Puzzles = listOf(
    Puzzle("wordle", "Wordle", "https://www.nytimes.com/games/wordle/index.html", "nytimes.com", "/games/wordle", listOf("wordle"), "guesses"),
    Puzzle("connections", "Connections", "https://www.nytimes.com/games/connections", "nytimes.com", "/games/connections", listOf("connections"), "mistakes"),
    Puzzle("daily-orbs", "Daily Orbs", "https://dailyorbs.com", "dailyorbs.com", "/", listOf("daily-orbs"), "guesses"),
    Puzzle("catfishing", "Catfishing", "https://catfishing.net", "catfishing.net", "/", listOf("catfishing"), "guesses"),
)

fun puzzleOf(id: String): Puzzle? = Puzzles.firstOrNull { it.id == id }

fun Puzzle.isOnThisSite(url: String): Boolean {
    val withoutScheme = url.substringAfter("://", "")
    val hostPart = withoutScheme.substringBefore('/').substringBefore(':')
    val path = "/" + withoutScheme.substringAfter('/', "").substringBefore('?').substringBefore('#')

    return (hostPart == host || hostPart.endsWith(".$host")) && path.startsWith(pathPrefix)
}

fun Puzzle.readingScript(): String {
    val markers = storageMarkers.joinToString(",") { "\"$it\"" }

    return "(function(){var out={};for(var i=0;i<localStorage.length;i++){var k=localStorage.key(i);if([$markers].some(function(m){return k.toLowerCase().indexOf(m)>=0})){out[k]=localStorage.getItem(k)}}return JSON.stringify(out)})()"
}

fun parseGameState(puzzle: Puzzle, storageJson: String, isToday: (Long) -> Boolean): PuzzleResult? {
    val storage = runCatching { Json.parseToJsonElement(storageJson).jsonObject }.getOrNull() ?: return null

    return when (puzzle.id) {
        "wordle" -> parseWordle(storage, isToday)
        else -> null
    }
}

private fun parseWordle(storage: JsonObject, isToday: (Long) -> Boolean): PuzzleResult? {
    val raw = storage["nyt-wordle-state"]?.jsonPrimitive?.content ?: return null
    val state = runCatching { Json.parseToJsonElement(raw).jsonObject }.getOrNull() ?: return null
    val playedAt = state["lastPlayedTs"]?.jsonPrimitive?.longOrNull ?: return null

    if (!isToday(playedAt)) {
        return null
    }

    val guesses = state["rowIndex"]?.jsonPrimitive?.intOrNull ?: return null

    return when (state["gameStatus"]?.jsonPrimitive?.content) {
        "WIN" -> PuzzleResult(solved = true, guesses = guesses)
        "FAIL" -> PuzzleResult(solved = false, guesses = guesses)
        else -> if (guesses > 0) PuzzleResult(guesses = guesses) else null
    }
}

private fun plural(count: Int, word: String): String = "$count $word${if (count == 1) "" else "s"}"

fun puzzleStatus(result: PuzzleResult?): String = when {
    result == null -> "Not played"
    result.solved == true && result.guesses != null -> "Solved in ${result.guesses} ${if (result.guesses == 1) "guess" else "guesses"}"
    result.solved == true && (result.mistakes ?: 0) > 0 -> "Solved, ${plural(result.mistakes!!, "mistake")}"
    result.solved == true -> "Solved"
    result.solved == false -> "Not solved"
    result.score != null -> "Played, ${result.score}"
    else -> "Played"
}

fun PuzzleResult.sourceLabel(): String = if (source == "read") "Read from the game" else "Ticked on the phone"
