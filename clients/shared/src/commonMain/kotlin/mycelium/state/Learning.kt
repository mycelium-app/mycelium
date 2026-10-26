package mycelium.state

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import mycelium.api.Exercise
import mycelium.api.PatchReflectionRequest
import mycelium.api.Reflection
import mycelium.api.ReflectionSchedule
import mycelium.api.ReflectionScheduleRequest
import mycelium.api.Skill
import mycelium.api.SolutionFile
import mycelium.api.MyceliumApi

enum class ReflectionSection(val key: String, val title: String) {
    Done("done", "Done"),
    Learned("learned", "Learned"),
    Waiting("waiting", "Waiting"),
    NextWeek("next_week", "Next week"),
}

fun Reflection.text(section: ReflectionSection): String = when (section) {
    ReflectionSection.Done -> done
    ReflectionSection.Learned -> learned
    ReflectionSection.Waiting -> waiting
    ReflectionSection.NextWeek -> nextWeek
}

fun Reflection.wasEdited(section: ReflectionSection): Boolean = section.key in edited

fun reflectionPatch(section: ReflectionSection, text: String): PatchReflectionRequest = when (section) {
    ReflectionSection.Done -> PatchReflectionRequest(done = text)
    ReflectionSection.Learned -> PatchReflectionRequest(learned = text)
    ReflectionSection.Waiting -> PatchReflectionRequest(waiting = text)
    ReflectionSection.NextWeek -> PatchReflectionRequest(nextWeek = text)
}

private val MonthNames = listOf("January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December")

fun dayLabel(isoDate: String, today: String): String {
    if (isoDate == today) {
        return "Today"
    }

    val (_, month, day) = isoDate.split('-').map { it.toInt() }

    return "$day ${MonthNames[month - 1]}"
}

fun weekLabel(isoDate: String): String {
    val (year, month, day) = isoDate.split('-').map { it.toInt() }

    return "Week of $day ${MonthNames[month - 1]} $year"
}

data class LearningState(
    val reflections: List<Reflection> = emptyList(),
    val schedule: ReflectionSchedule? = null,
    val loaded: Boolean = false,
    val redrafting: Set<String> = emptySet(),
    val problem: String? = null,
    val skills: List<Skill> = emptyList(),
    val exercises: List<Exercise> = emptyList(),
    val suggested: List<Exercise> = emptyList(),
    val working: Set<String> = emptySet(),
    val solutions: Map<String, List<SolutionFile>> = emptyMap(),
)

fun List<Exercise>.practisedOf(skill: Skill): List<Exercise> = filter { it.kind == skill.kind && it.name == skill.name }

fun List<Exercise>.doneCount(): Int = count { it.state == "done" }

private fun List<Exercise>.replacing(updated: Exercise): List<Exercise> = if (any { it.id == updated.id }) map { if (it.id == updated.id) updated else it } else listOf(updated) + this

class LearningStore(private val api: MyceliumApi, private val scope: CoroutineScope) {
    private val mutableState = MutableStateFlow(LearningState())

    val state: StateFlow<LearningState> = mutableState.asStateFlow()

    suspend fun load() {
        val reflections = runCatching { api.reflections() }.getOrNull() ?: return
        val schedule = runCatching { api.reflectionSchedule() }.getOrNull()

        mutableState.update { it.copy(reflections = reflections, schedule = schedule, loaded = true) }
    }

    suspend fun loadSkills() {
        val skills = runCatching { api.skills() }.getOrNull() ?: return
        val exercises = runCatching { api.exercises() }.getOrNull().orEmpty()
        val suggested = runCatching { api.suggestedExercises() }.getOrNull().orEmpty()

        mutableState.update { it.copy(skills = skills, exercises = exercises, suggested = suggested, loaded = true) }
    }

    fun reload() {
        scope.launch {
            load()
            loadSkills()
        }
    }

    fun hideSkill(skill: Skill) {
        scope.launch {
            val hidden = runCatching { api.hideSkill(skill) }

            mutableState.update { state ->
                if (hidden.isSuccess) {
                    state.copy(skills = state.skills - skill, problem = null)
                } else {
                    state.copy(problem = "Could not hide ${skill.name}. Check your connection and try again.")
                }
            }
        }
    }

    fun startExercise(exercise: Exercise) = change(exercise, "start the exercise") { api.startExercise(exercise.id) }

    fun postponeExercise(exercise: Exercise) = change(exercise, "hide the exercise until tomorrow") { api.exerciseNotNow(exercise.id) }

    fun requestReview(exercise: Exercise) = change(exercise, "ask for a review") { api.reviewExercise(exercise.id) }

    fun finishExercise(exercise: Exercise) = change(exercise, "mark the exercise done") { api.finishExercise(exercise.id) }

    fun runTests(exercise: Exercise) = change(exercise, "run the tests") { api.runExerciseTests(exercise.id).let { tests -> exercise.copy(tests = tests) } }

    private fun change(exercise: Exercise, what: String, call: suspend () -> Exercise) {
        mutableState.update { it.copy(working = it.working + exercise.id, problem = null) }
        scope.launch {
            val result = runCatching { call() }
            val updated = result.getOrNull()

            mutableState.update { state ->
                state.copy(
                    working = state.working - exercise.id,
                    exercises = if (updated == null) state.exercises else state.exercises.replacing(updated),
                    suggested = if (updated == null) state.suggested else state.suggested.filter { it.id != updated.id },
                    problem = result.exceptionOrNull()?.let { "Could not $what. ${it.message.orEmpty()}".trim() },
                )
            }
        }
    }

    fun save(reflection: Reflection, section: ReflectionSection, text: String) {
        if (text.trim() == reflection.text(section).trim()) {
            return
        }

        scope.launch {
            val saved = runCatching { api.patchReflection(reflection.id, reflectionPatch(section, text)) }.getOrNull()

            if (saved == null) {
                mutableState.update { it.copy(problem = "Could not save your changes to ${section.title}. Check your connection and try again.") }

                return@launch
            }

            mutableState.update { state -> state.copy(reflections = state.reflections.map { if (it.id == saved.id) saved else it }, problem = null) }
        }
    }

    fun redraft(reflection: Reflection, onStarted: (String) -> Unit = {}) {
        mutableState.update { it.copy(redrafting = it.redrafting + reflection.id, problem = null) }
        scope.launch {
            val started = runCatching { api.redraftReflection(reflection.id) }

            mutableState.update { state ->
                state.copy(redrafting = state.redrafting - reflection.id, problem = started.exceptionOrNull()?.let { "Could not start a redraft. ${it.message.orEmpty()}".trim() })
            }
            started.getOrNull()?.let { onStarted(it.conversationId) }
        }
    }

    fun setUp(request: ReflectionScheduleRequest) {
        scope.launch {
            val saved = runCatching { api.saveReflectionSchedule(request) }

            mutableState.update { state ->
                state.copy(schedule = saved.getOrNull() ?: state.schedule, problem = saved.exceptionOrNull()?.let { "Could not set up the weekly reflection. ${it.message.orEmpty()}".trim() })
            }
        }
    }
}
