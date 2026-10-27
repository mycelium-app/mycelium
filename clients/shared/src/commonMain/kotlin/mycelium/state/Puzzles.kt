package mycelium.state

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.time.Instant
import mycelium.api.PuzzleResult

data class Puzzle(val id: String, val name: String, val url: String, val host: String, val pathPrefix: String, val storageKeyPattern: String, val measure: String)

val Puzzles = listOf(
    Puzzle("wordle", "Wordle", "https://www.nytimes.com/games/wordle/index.html", "nytimes.com", "/games/wordle", "^games-state-wordle(V2|-collection)/", "guesses"),
    Puzzle("connections", "Connections", "https://www.nytimes.com/games/connections", "nytimes.com", "/games/connections", "^games-state-connections(-collection)?/", "mistakes"),
    Puzzle("daily-orbs", "Daily Orbs", "https://dailyorbs.com", "dailyorbs.com", "/", "daily-orbs-\\d{4}-\\d{2}-\\d{2}$", "guesses"),
    Puzzle("catfishing", "Catfishing", "https://catfishing.net", "catfishing.net", "/", "^game-\\d+$", "guesses"),
)

fun puzzleOf(id: String): Puzzle? = Puzzles.firstOrNull { it.id == id }

fun Puzzle.isOnThisSite(url: String): Boolean {
    val withoutScheme = url.substringAfter("://", "")
    val hostPart = withoutScheme.substringBefore('/').substringBefore(':')
    val path = "/" + withoutScheme.substringAfter('/', "").substringBefore('?').substringBefore('#')

    return (hostPart == host || hostPart.endsWith(".$host")) && path.startsWith(pathPrefix)
}

fun Puzzle.readingScript(): String {
    val pattern = storageKeyPattern.replace("\\", "\\\\")

    return "(function(){var out={};var pattern=new RegExp(\"$pattern\");for(var i=0;i<localStorage.length;i++){var k=localStorage.key(i);if(pattern.test(k)){out[k]=localStorage.getItem(k)}}return JSON.stringify(out)})()"
}

sealed interface GameReading {
    data object NoStorage : GameReading

    data object Unrecognised : GameReading

    data class Read(val result: PuzzleResult?) : GameReading
}

private const val CatfishingFirstDay = "2024-06-23"
private const val CatfishingQuestions = 10
private const val OrbsToWin = 6
private const val OrbsPuzzlesPerDay = 4
private const val SecondsPerDay = 86_400L

fun parseGameState(puzzle: Puzzle, storageJson: String, today: String): GameReading {
    val storage = runCatching { Json.parseToJsonElement(storageJson).jsonObject }.getOrNull() ?: return GameReading.Unrecognised

    if (storage.isEmpty()) {
        return GameReading.NoStorage
    }

    val values = storage.mapValues { (_, raw) -> runCatching { Json.parseToJsonElement(raw.jsonPrimitive.content).jsonObject }.getOrNull() }
    val recognisedValues = when (puzzle.id) {
        "wordle", "connections" -> values.filterValues { it?.get("states") is JsonArray }
        "daily-orbs" -> values.filterValues { it?.get("puzzles") is JsonArray }
        "catfishing" -> values.filterValues { it?.get("s")?.jsonPrimitive?.intOrNull != null }
        else -> emptyMap()
    }

    if (recognisedValues.isEmpty()) {
        return GameReading.Unrecognised
    }

    val result = when (puzzle.id) {
        "wordle" -> recognisedValues.values.firstNotNullOfOrNull { nytResult(it, today, ::wordleResult) }
        "connections" -> recognisedValues.values.firstNotNullOfOrNull { nytResult(it, today, ::connectionsResult) }
        "daily-orbs" -> recognisedValues.entries.firstOrNull { it.key.endsWith("daily-orbs-$today") }?.value?.let(::orbsResult)
        else -> recognisedValues.entries.firstOrNull { it.key == "game-${catfishingDay(today)}" }?.value?.let(::catfishingResult)
    }

    return GameReading.Read(result)
}

private fun nytResult(storage: JsonObject?, today: String, result: (JsonObject) -> PuzzleResult?): PuzzleResult? {
    val states = storage?.get("states") as? JsonArray ?: return null

    return states.asSequence()
        .mapNotNull { it as? JsonObject }
        .filter { it["printDate"]?.jsonPrimitive?.contentOrNull == today }
        .mapNotNull { it["data"] as? JsonObject }
        .filter { it["isPlayingArchive"]?.jsonPrimitive?.booleanOrNull != true }
        .firstNotNullOfOrNull(result)
}

private fun wordleResult(data: JsonObject): PuzzleResult? {
    val guesses = (data["boardState"] as? JsonArray).orEmpty().count { it.jsonPrimitive.content.isNotEmpty() }

    return when (data["status"]?.jsonPrimitive?.contentOrNull) {
        "WIN" -> PuzzleResult(solved = true, guesses = guesses)
        "FAIL" -> PuzzleResult(solved = false, guesses = guesses)
        else -> null
    }
}

private fun connectionsResult(data: JsonObject): PuzzleResult? {
    if (data["puzzleComplete"]?.jsonPrimitive?.booleanOrNull != true) {
        return null
    }

    val mistakes = data["mistakes"]?.jsonPrimitive?.intOrNull ?: 0

    return PuzzleResult(solved = data["puzzleWon"]?.jsonPrimitive?.booleanOrNull == true, mistakes = mistakes)
}

private fun orbsResult(day: JsonObject): PuzzleResult? {
    val events = day["eventHistory"] as? JsonArray ?: JsonArray(emptyList())
    val orbs = events.count { (it as? JsonObject)?.get("type")?.jsonPrimitive?.contentOrNull == "orb" }
    val puzzles = day["puzzles"] as? JsonArray ?: return null
    val allComplete = puzzles.size == OrbsPuzzlesPerDay && puzzles.all { (it as? JsonObject)?.get("completed")?.jsonPrimitive?.booleanOrNull == true }
    val won = orbs >= OrbsToWin

    return if (won || allComplete) PuzzleResult(solved = won, score = plural(orbs, "orb")) else null
}

private fun catfishingDay(today: String): Long {
    val first = Instant.parse("${CatfishingFirstDay}T00:00:00Z").epochSeconds
    val date = Instant.parse("${today}T00:00:00Z").epochSeconds

    return (date - first) / SecondsPerDay
}

private fun catfishingResult(game: JsonObject): PuzzleResult? {
    val completed = game["s"]?.jsonPrimitive?.intOrNull == 1
    val corrects = (game["c"] as? JsonArray)?.firstOrNull()?.jsonPrimitive?.intOrNull

    return if (completed && corrects != null) PuzzleResult(score = "$corrects of $CatfishingQuestions") else null
}

private fun plural(count: Int, word: String): String = "$count $word${if (count == 1) "" else "s"}"

fun puzzleStatus(result: PuzzleResult?): String = when {
    result == null -> "Not played"
    result.solved == true && result.guesses != null -> "Solved in ${result.guesses} ${if (result.guesses == 1) "guess" else "guesses"}"
    result.solved == true && (result.mistakes ?: 0) > 0 -> "Solved, ${plural(result.mistakes!!, "mistake")}"
    result.solved == true && result.score != null -> "Solved, ${result.score}"
    result.solved == true -> "Solved"
    result.solved == false && result.score != null -> "Not solved, ${result.score}"
    result.solved == false -> "Not solved"
    result.score != null -> "Played, ${result.score}"
    else -> "Played"
}

fun PuzzleResult.sourceLabel(): String = if (source == "read") "Read from the game" else "Ticked on the phone"
