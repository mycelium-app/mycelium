package mycelium.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.time.Clock
import mycelium.api.Exercise
import mycelium.api.ExerciseTests
import mycelium.api.Skill
import mycelium.state.Store
import mycelium.state.doneCount
import mycelium.state.practisedOf
import mycelium.state.dayLabel

private val SparklineWidth = 160.dp
private val SparklineHeight = 20.dp
private val ShareBarWidth = 90.dp
private val ShareBarHeight = 6.dp
private val SkillKinds = listOf("language" to "Languages", "area" to "Areas of the codebase", "other" to "Other")

private fun today(): String = Clock.System.now().toString().take(10)

fun sharePercent(skill: Skill): String? = skill.ownShare?.let { "${(it * 100).toInt()}%" }

fun exerciseStateName(state: String): String = when (state) {
    "started" -> "Started"
    "reviewed" -> "Reviewed"
    "done" -> "Done"
    else -> "Not started"
}

private fun exerciseStateIcon(state: String): MyIcon = when (state) {
    "done" -> MyIcon.Check
    "reviewed" -> MyIcon.Review
    "started" -> MyIcon.Pencil
    else -> MyIcon.Plus
}

fun testsSummary(tests: ExerciseTests): String = when {
    tests.timedOut -> "The tests timed out."
    tests.failed == 0 && tests.exit == 0 -> "Tests: ${tests.passed?.let { "$it passed" } ?: "all passed"}."
    else -> "Tests: ${tests.passed?.let { "$it passed, " }.orEmpty()}${tests.failed} failed${tests.failing.firstOrNull()?.let { " ($it)" }.orEmpty()}."
}

@Composable
fun SkillsTab(store: Store) {
    val learning by store.learning.state.collectAsState()
    var chosen by remember { mutableStateOf<String?>(null) }
    val skills = learning.skills
    val selected = skills.firstOrNull { "${it.kind}/${it.name}" == chosen } ?: skills.firstOrNull()

    LaunchedEffect(Unit) {
        store.learning.loadSkills()
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        learning.problem?.let { Label(it, Modifier.semantics { liveRegion = LiveRegionMode.Assertive }, Tokens.Type.MetaStrong, MyTheme.colors.statusFailed) }

        if (skills.isEmpty()) {
            Label(if (learning.loaded) "Skills appear here once you or an agent have changed code on one of your nodes." else "Loading skills.", color = MyTheme.colors.textMuted)

            return@Column
        }

        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val wide = maxWidth >= 900.dp
            val list: @Composable (Modifier) -> Unit = { modifier ->
                Column(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                    for ((kind, title) in SkillKinds) {
                        val group = skills.filter { it.kind == kind }

                        if (group.isNotEmpty()) {
                            SkillGroup(title, group, learning.exercises, selected) { chosen = "${it.kind}/${it.name}" }
                        }
                    }

                    Label("From the last 90 days of changes on your nodes. Share of changed lines.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
                }
            }

            if (wide) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
                    list(Modifier.width(440.dp))
                    selected?.let { SkillDetail(store, it, Modifier.weight(1f)) }
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
                    list(Modifier.fillMaxWidth())
                    selected?.let { SkillDetail(store, it, Modifier.fillMaxWidth()) }
                }
            }
        }
    }
}

@Composable
private fun SkillGroup(title: String, group: List<Skill>, exercises: List<Exercise>, selected: Skill?, onSelect: (Skill) -> Unit) {
    val colors = MyTheme.colors

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
        Label(title, Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)

        Table(
            listOf(Col("Skill", 120.dp, weight = 1f), Col("Written by you", 130.dp, preferred = 130.dp), Col("Exercises", 80.dp, preferred = 90.dp)),
            group.size,
            { row, column ->
                val skill = group[row]
                val related = exercises.practisedOf(skill)

                when (column) {
                    0 -> TableCell.Text(skill.name)
                    1 -> TableCell.Slot("Written by you", 130.dp) { ShareBar(skill) }
                    else -> TableCell.Text(if (related.isEmpty()) "None yet" else "${related.doneCount()} of ${related.size}", color = colors.textMuted)
                }
            },
            rowKey = { "${group[it].kind}/${group[it].name}" },
            onRow = { onSelect(group[it]) },
            rowDescription = { group[it].name },
            rowModifier = { if (group[it] == selected) Modifier.background(colors.bgSelected) else Modifier },
        )
    }
}

@Composable
private fun ShareBar(skill: Skill) {
    val colors = MyTheme.colors
    val share = (skill.ownShare ?: 0.0).toFloat().coerceIn(0f, 1f)

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
        Canvas(Modifier.width(ShareBarWidth).height(ShareBarHeight).clearAndSetSemantics {}) {
            drawRoundRect(colors.border, size = size, cornerRadius = androidx.compose.ui.geometry.CornerRadius(size.height / 2f))
            drawRoundRect(colors.accent, size = androidx.compose.ui.geometry.Size(size.width * share, size.height), cornerRadius = androidx.compose.ui.geometry.CornerRadius(size.height / 2f))
        }
        Label(sharePercent(skill) ?: "None", style = Tokens.Type.Ui, color = colors.textMuted)
    }
}

@Composable
private fun SkillDetail(store: Store, skill: Skill, modifier: Modifier) {
    val learning by store.learning.state.collectAsState()
    val related = learning.exercises.practisedOf(skill)
    var chosen by remember(skill.name) { mutableStateOf<String?>(null) }
    val exercise = related.firstOrNull { it.id == chosen } ?: related.firstOrNull()
    val colors = MyTheme.colors

    Column(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label(skill.name, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
            MyButton({ store.learning.hideSkill(skill) }, text = "Hide skill", description = "Hide ${skill.name}", muted = true)
        }

        if (related.isEmpty()) {
            Label("No exercises for ${skill.name} yet. They are suggested from code an agent wrote for you recently.", color = colors.textMuted)

            return@Column
        }

        Table(
            listOf(Col("Exercise", 140.dp, weight = 1f), Col("State", 100.dp, preferred = 110.dp), Col("From", 80.dp, preferred = 100.dp, priority = 1)),
            related.size,
            { row, column ->
                val item = related[row]

                when (column) {
                    0 -> TableCell.Text(item.title)
                    1 -> TableCell.Slot(exerciseStateName(item.state), Tokens.Size.IconSm + Tokens.Space.S1_5) {
                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                            Icon(exerciseStateIcon(item.state), colors.textMuted, Tokens.Size.IconSm)
                            Label(exerciseStateName(item.state), color = colors.textMuted)
                        }
                    }
                    else -> TableCell.Text(dayLabel((item.startedAt ?: item.createdAt).take(10), today()), color = colors.textMuted)
                }
            },
            rowKey = { related[it].id },
            onRow = { chosen = related[it].id },
            rowDescription = { related[it].title },
            rowModifier = { if (related[it] == exercise) Modifier.background(colors.bgSelected) else Modifier },
        )

        exercise?.let { ExerciseCard(store, it, Modifier.fillMaxWidth()) }
    }
}

@Composable
fun ExerciseCard(store: Store, exercise: Exercise, modifier: Modifier = Modifier) {
    val learning by store.learning.state.collectAsState()
    val colors = MyTheme.colors
    val busy = exercise.id in learning.working
    val tests = exercise.tests
    val review = exercise.review
    val started = exercise.conversationId != null && exercise.state != "suggested"

    Column(modifier.border(Tokens.Size.Hairline, colors.border, ControlShape).background(colors.bgPanel, ControlShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), itemVerticalAlignment = Alignment.CenterVertically) {
            Label(exercise.title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)
            exercise.branch?.let { Tag(it) }
        }

        Label(exercise.brief, color = colors.textMuted)

        if (tests != null) {
            Label(testsSummary(tests), Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Ui, if (tests.failed == 0 && tests.exit == 0 && !tests.timedOut) colors.text else colors.statusAttention)
        }

        if (exercise.reviewing) {
            Label("An agent is reviewing your branch.", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, color = colors.textMuted)
        }

        if (review != null) {
            Column(Modifier.semantics(mergeDescendants = false) { contentDescription = "Review of ${exercise.title}" }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                Label("Review", Modifier.semantics { heading() }, Tokens.Type.MetaStrong, colors.textMuted)
                Label(review.summary)

                for (comment in review.comments) {
                    Column(Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, ControlShape).padding(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                        Label("${comment.path}:${comment.line}${if (comment.praise) ", good" else ""}", style = Tokens.Type.Meta, color = colors.textMuted)
                        Label(comment.body)
                    }
                }
            }
        }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            if (!started) {
                MyButton({ store.learning.startExercise(exercise) }, text = if (busy) "Starting" else "Start exercise", variant = ButtonVariant.Primary)
            } else {
                MyButton({ store.learning.runTests(exercise) }, text = "Run tests", description = "Run tests for ${exercise.title}", variant = ButtonVariant.Outline)
                MyButton({ store.learning.requestReview(exercise) }, text = if (review == null) "Ask for review" else "Review again", description = "Ask for a review of ${exercise.title}", variant = ButtonVariant.Outline)

                if (exercise.state != "done") {
                    MyButton({ store.learning.finishExercise(exercise) }, text = "Mark done", description = "Mark ${exercise.title} done", variant = ButtonVariant.Outline)
                }

                exercise.conversationId?.let { id -> MyButton({ store.act { store.open(id) } }, text = "Open the branch chat", description = "Open the chat for ${exercise.title}", variant = ButtonVariant.Primary) }
            }
        }
    }
}

@Composable
fun SuggestedExerciseCard(store: Store, modifier: Modifier = Modifier) {
    val learning by store.learning.state.collectAsState()
    val exercise = learning.suggested.firstOrNull() ?: learning.exercises.firstOrNull { it.state == "started" }
    val colors = MyTheme.colors

    if (exercise == null) {
        return
    }

    val started = exercise.state != "suggested"

    Column(modifier.border(Tokens.Size.Hairline, colors.border, ControlShape).background(colors.bgPanel, ControlShape).padding(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(MyIcon.Pencil, colors.accent, Tokens.Size.IconMd)
            Label(if (started) "Exercise in progress" else "Suggested exercise", Modifier.semantics { heading() }, Tokens.Type.UiStrong)
        }

        Label(exercise.title, style = Tokens.Type.UiStrong)
        Label(exercise.brief, color = colors.textMuted)

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            Tag(exercise.name)
            exercise.minutes?.let { Tag("About $it min") }
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            if (started) {
                exercise.conversationId?.let { id -> MyButton({ store.act { store.open(id) } }, text = "Open the branch chat", variant = ButtonVariant.Primary) }
            } else {
                MyButton({ store.learning.postponeExercise(exercise) }, text = "Not now", description = "Hide ${exercise.title} until tomorrow", muted = true)
                MyButton({ store.learning.startExercise(exercise) }, text = "Start exercise", description = "Start ${exercise.title}", variant = ButtonVariant.Primary)
            }
        }
    }
}

@Composable
fun SkillsOverTime(store: Store, onAll: () -> Unit, modifier: Modifier = Modifier) {
    val learning by store.learning.state.collectAsState()
    val skills = learning.skills.sortedByDescending { it.weeklyOwnLines.sum() }.take(5)
    val colors = MyTheme.colors

    if (skills.isEmpty()) {
        return
    }

    Column(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Label("Skills over time", Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong)
            MyButton(onAll, text = "All skills", trailingIcon = MyIcon.ChevronRight, muted = true)
        }

        Table(
            listOf(Col("Skill", 120.dp, weight = 1f), Col("Lines written by you, last 8 weeks", 120.dp, preferred = SparklineWidth + Tokens.Space.S3), Col("Last practised", 90.dp, preferred = 110.dp, priority = 1)),
            skills.size,
            { row, column ->
                val skill = skills[row]

                when (column) {
                    0 -> TableCell.Text(skill.name)
                    1 -> TableCell.Slot("Lines written by you, last 8 weeks") { Sparkline(skill) }
                    else -> TableCell.Text(skill.lastPractised?.let { dayLabel(it, today()) } ?: "Not yet", color = colors.textMuted)
                }
            },
            rowKey = { "${skills[it].kind}/${skills[it].name}" },
            rowDescription = { skills[it].name },
        )
    }
}

@Composable
private fun Sparkline(skill: Skill) {
    val colors = MyTheme.colors
    val weeks = skill.weeklyOwnLines
    val peak = weeks.maxOrNull()?.coerceAtLeast(1L) ?: 1L

    Canvas(Modifier.width(SparklineWidth).height(SparklineHeight).clearAndSetSemantics { contentDescription = "${skill.name}, lines you wrote each week, oldest first: ${weeks.joinToString(", ")}" }) {
        val step = if (weeks.size > 1) size.width / (weeks.size - 1) else 0f
        val points = weeks.mapIndexed { index, lines -> Offset(index * step, size.height - 2f - (size.height - 4f) * lines.toFloat() / peak) }

        points.zipWithNext().forEach { (from, to) -> drawLine(colors.accent, from, to, strokeWidth = 3f, cap = StrokeCap.Round) }
    }
}
