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

data class Puzzle(val id: String, val name: String, val url: String, val host: String, val pathPrefix: String, val storageKeyPattern: String, val measure: String, val native: Boolean = false)

val Puzzles = listOf(
    Puzzle("wordle", "Wordle", "https://www.nytimes.com/games/wordle/index.html", "nytimes.com", "/games/wordle", "^games-state-wordle(V2|-collection)/", "guesses", native = true),
    Puzzle("connections", "Connections", "https://www.nytimes.com/games/connections", "nytimes.com", "/games/connections", "^games-state-connections(-collection)?/", "mistakes", native = true),
    Puzzle("daily-orbs", "Daily Orbs", "https://dailyorbs.com", "dailyorbs.com", "/", "daily-orbs-\\d{4}-\\d{2}-\\d{2}$", "orbs"),
    Puzzle("catfishing", "Catfishing", "https://catfishing.net", "catfishing.net", "/", "^game-\\d+$", "correct"),
    Puzzle("enclose-horse", "enclose.horse", "https://enclose.horse", "enclose.horse", "/", "^enclose_submissions$", "points"),
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

    val levels = if (id == "enclose-horse") "out[\"$EncloseLevelsKey\"]=JSON.stringify(window.__DAILY_LEVELS__||[]);" else ""

    return "(function(){var out={};var pattern=new RegExp(\"$pattern\");for(var i=0;i<localStorage.length;i++){var k=localStorage.key(i);if(pattern.test(k)){out[k]=localStorage.getItem(k)}}${levels}return JSON.stringify(out)})()"
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
private const val EncloseSubmissionsKey = "enclose_submissions"
private const val EncloseLevelsKey = "__daily_levels__"

fun parseGameState(puzzle: Puzzle, storageJson: String, today: String): GameReading {
    val storage = runCatching { Json.parseToJsonElement(storageJson).jsonObject }.getOrNull() ?: return GameReading.Unrecognised

    if (storage.isEmpty()) {
        return GameReading.NoStorage
    }

    if (puzzle.id == "enclose-horse") {
        return encloseReading(storage, today)
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

private fun encloseReading(storage: JsonObject, today: String): GameReading {
    val submissionsText = storage[EncloseSubmissionsKey]?.jsonPrimitive?.contentOrNull ?: return GameReading.NoStorage
    val submissions = runCatching { Json.parseToJsonElement(submissionsText).jsonObject }.getOrNull() ?: return GameReading.Unrecognised
    val levelsText = storage[EncloseLevelsKey]?.jsonPrimitive?.contentOrNull ?: return GameReading.Unrecognised
    val levels = runCatching { Json.parseToJsonElement(levelsText) as? JsonArray }.getOrNull() ?: return GameReading.Unrecognised
    val dailyId = levels.asSequence()
        .mapNotNull { it as? JsonObject }
        .firstOrNull { it["date"]?.jsonPrimitive?.contentOrNull == today }
        ?.get("id")?.jsonPrimitive?.contentOrNull
    val points = dailyId?.let { (submissions[it] as? JsonObject)?.get("score")?.jsonPrimitive?.intOrNull }

    return GameReading.Read(points?.let { PuzzleResult(score = it) })
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
        "WIN" -> PuzzleResult(solved = true, score = guesses)
        "FAIL" -> PuzzleResult(solved = false)
        else -> null
    }
}

private fun connectionsResult(data: JsonObject): PuzzleResult? {
    if (data["puzzleComplete"]?.jsonPrimitive?.booleanOrNull != true) {
        return null
    }

    val mistakes = data["mistakes"]?.jsonPrimitive?.intOrNull ?: 0

    return PuzzleResult(solved = data["puzzleWon"]?.jsonPrimitive?.booleanOrNull == true, score = mistakes)
}

private fun orbsResult(day: JsonObject): PuzzleResult? {
    val events = day["eventHistory"] as? JsonArray ?: JsonArray(emptyList())
    val orbs = events.count { (it as? JsonObject)?.get("type")?.jsonPrimitive?.contentOrNull == "orb" }
    val puzzles = day["puzzles"] as? JsonArray ?: return null
    val allComplete = puzzles.size == OrbsPuzzlesPerDay && puzzles.all { (it as? JsonObject)?.get("completed")?.jsonPrimitive?.booleanOrNull == true }
    val won = orbs >= OrbsToWin

    return if (won || allComplete) PuzzleResult(solved = won, score = orbs) else null
}

private fun catfishingDay(today: String): Long {
    val first = Instant.parse("${CatfishingFirstDay}T00:00:00Z").epochSeconds
    val date = Instant.parse("${today}T00:00:00Z").epochSeconds

    return (date - first) / SecondsPerDay
}

private fun catfishingResult(game: JsonObject): PuzzleResult? {
    val completed = game["s"]?.jsonPrimitive?.intOrNull == 1
    val corrects = (game["c"] as? JsonArray)?.firstOrNull()?.jsonPrimitive?.intOrNull

    return if (completed && corrects != null) PuzzleResult(score = corrects) else null
}

private fun plural(count: Int, word: String, many: String = "${word}s"): String = "$count ${if (count == 1) word else many}"

fun Puzzle.scoreLabel(result: PuzzleResult): String? {
    val score = result.score

    return when (measure) {
        "guesses" -> if (score != null) plural(score, "guess", "guesses") else if (result.solved == false) "X" else null
        "mistakes" -> if (score == null) null else if (score == 0) "no mistakes" else plural(score, "mistake")
        "orbs" -> score?.let { plural(it, "orb") }
        "correct" -> score?.let { "$it of $CatfishingQuestions" }
        else -> score?.let { plural(it, "point") }
    }
}

fun puzzleStatus(puzzle: Puzzle, result: PuzzleResult?): String {
    val label = result?.let { puzzle.scoreLabel(it) }

    return when {
        result == null -> "Not played"
        result.solved == true && puzzle.measure == "guesses" && label != null -> "Solved in $label"
        result.solved == true && label != null -> "Solved, $label"
        result.solved == true -> "Solved"
        result.solved == false && puzzle.measure != "guesses" && label != null -> "Not solved, $label"
        result.solved == false -> "Not solved"
        label != null -> "Played, $label"
        else -> "Played"
    }
}

fun PuzzleResult.sourceLabel(): String = when (source) {
    "read" -> "Read from the game"
    "native" -> "Played here"
    else -> "Entered by hand"
}
