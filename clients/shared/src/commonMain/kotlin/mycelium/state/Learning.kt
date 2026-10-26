package mycelium.state

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import mycelium.api.PatchReflectionRequest
import mycelium.api.Reflection
import mycelium.api.ReflectionSchedule
import mycelium.api.ReflectionScheduleRequest
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
)

class LearningStore(private val api: MyceliumApi, private val scope: CoroutineScope) {
    private val mutableState = MutableStateFlow(LearningState())

    val state: StateFlow<LearningState> = mutableState.asStateFlow()

    suspend fun load() {
        val reflections = runCatching { api.reflections() }.getOrNull() ?: return
        val schedule = runCatching { api.reflectionSchedule() }.getOrNull()

        mutableState.update { it.copy(reflections = reflections, schedule = schedule, loaded = true) }
    }

    fun reload() {
        scope.launch { load() }
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
