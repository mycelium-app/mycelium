package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlin.time.Clock
import kotlin.time.Instant
import mycelium.Platform
import mycelium.api.WellnessDay
import mycelium.state.BreakPhase
import mycelium.state.PomodoroFocusMinutes
import mycelium.state.Puzzles
import mycelium.state.Store
import mycelium.state.WellnessState
import mycelium.state.weekdayOf
import mycelium.state.WeekdayNames

private const val WellnessTabName = "Overview"
private const val SkillsTabName = "Skills"
private const val ReflectionsTabName = "Reflections"
private const val MinuteMillis = 60_000L
private val ChartHeight = 120.dp

fun durationText(minutes: Long): String = when {
    minutes <= 0 -> "None"
    minutes < 60 -> "$minutes min"
    else -> "${minutes / 60} h ${minutes % 60} min"
}

fun countdownText(seconds: Long): String {
    val left = seconds.coerceAtLeast(0)

    return "${(left / 60).toString().padStart(2, '0')}:${(left % 60).toString().padStart(2, '0')}"
}

@Composable
fun WellnessPresence(store: Store, platform: Platform) {
    val wellness by store.wellness.state.collectAsState()
    var focused by remember { mutableStateOf(platform.windowFocused()) }
    val due = wellness.clock.phase == BreakPhase.Due

    LaunchedEffect(store) {
        while (true) {
            delay(MinuteMillis)
            focused = platform.windowFocused()

            if (store.viewing != null && focused) {
                store.wellness.heartbeat(60)
                store.wellness.tickActiveMinute()
            }

            store.wellness.releaseHeld().takeIf { it.isNotEmpty() }?.let { released ->
                val single = released.singleOrNull()

                platform.notify(single?.title ?: "While you were away", if (single == null) "${released.size} agents finished" else "Finished", single?.conversationId.orEmpty(), false)
            }
        }
    }

    LaunchedEffect(due) {
        if (due && !platform.windowFocused()) {
            platform.breakNotification("Time for a break", "You have been at it for ${wellness.clock.activeMinutes} minutes.")
        }
    }

    Box(Modifier.fillMaxSize().padding(Tokens.Space.S4), contentAlignment = Alignment.BottomEnd) {
        BreakToast(
            wellness.clock,
            wellness.clock.breakLength(wellness.settings),
            onSnooze = store.wellness::snooze,
            onStartBreak = store.wellness::startBreak,
            onDismiss = store.wellness::dismiss,
        )
    }
}

@Composable
fun WellnessScreen(store: Store, platform: Platform, startOn: String = "overview") {
    val wellness by store.wellness.state.collectAsState()
    var tab by remember { mutableStateOf(if (startOn == "skills") SkillsTabName else WellnessTabName) }

    LaunchedEffect(Unit) {
        store.wellness.load()
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            Icon(MyIcon.Leaf, MyTheme.colors.accent, Tokens.Size.IconMd)
            BarTitle("Wellness & Learning")
        }

        TabRow(listOf(WellnessTabName, SkillsTabName, ReflectionsTabName), tab, { tab = it }, "Wellness and Learning sections", Modifier.padding(horizontal = Tokens.Space.S3))
        Hairline()

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
            if (tab == WellnessTabName) {
                WellnessOverview(wellness)
                LaunchedEffect(Unit) { store.learning.loadSkills() }
                SuggestedExerciseCard(store, Modifier.widthIn(max = Tokens.Size.Page))
                SkillsOverTime(store, { tab = SkillsTabName }, Modifier.widthIn(max = Tokens.Size.Page))
                PomodoroCard(store, wellness)
                PuzzlesCard(store, wellness, platform)
            } else if (tab == SkillsTabName) {
                SkillsTab(store)
            } else {
                ReflectionsTab(store)
            }
        }
    }
}

@Composable
private fun WellnessOverview(wellness: WellnessState) {
    val days = wellness.overview?.days.orEmpty()
    val today = days.lastOrNull()

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("Overview", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
            Stat("With agents today", durationText(today?.agentMinutes ?: 0))
            Stat("Looking at sessions today", durationText(today?.viewingMinutes ?: 0))
            Stat("Breaks today", if ((today?.breaks ?: 0L) == 0L) "None" else today?.breaks.toString())
        }

        if (days.isNotEmpty()) {
            WeekChart(days)
        }
    }
}

@Composable
private fun Stat(name: String, value: String) {
    val colors = MyTheme.colors

    Column(
        Modifier.widthIn(min = 160.dp).border(Tokens.Size.Hairline, colors.border, ControlShape).padding(Tokens.Space.S3).semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
    ) {
        Label(name, style = Tokens.Type.Meta, color = colors.textMuted)
        Label(value, style = Tokens.Type.Title)
    }
}

@Composable
private fun WeekChart(days: List<WellnessDay>) {
    val colors = MyTheme.colors
    val peak = days.maxOf { maxOf(it.agentMinutes, it.viewingMinutes) }.coerceAtLeast(1)
    val summary = days.joinToString(". ") { "${dayName(it.day)}: ${durationText(it.agentMinutes)} with agents, ${durationText(it.viewingMinutes)} looking at sessions" }

    Column(Modifier.fillMaxWidth().semantics { contentDescription = "Last 7 days. $summary" }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Last 7 days", style = Tokens.Type.Meta, color = colors.textMuted)

        Row(Modifier.fillMaxWidth().height(ChartHeight + Tokens.Space.S6), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Bottom) {
            for (day in days) {
                Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                    Row(Modifier.fillMaxWidth().height(ChartHeight), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5), verticalAlignment = Alignment.Bottom) {
                        Bars(day.agentMinutes, peak, colors.accent, Modifier.weight(1f))
                        Bars(day.viewingMinutes, peak, colors.textMuted, Modifier.weight(1f))
                    }
                    Label(dayName(day.day).take(3), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                }
            }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
            Legend("With agents", colors.accent)
            Legend("Looking at sessions", colors.textMuted)
        }
    }
}

@Composable
private fun Bars(minutes: Long, peak: Long, colour: androidx.compose.ui.graphics.Color, modifier: Modifier) {
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Bottom) {
        if (minutes > 0) {
            Label(minutes.toString(), Modifier.wrapContentWidth(unbounded = true), Tokens.Type.Meta, MyTheme.colors.textMuted, maxLines = 1)
        }

        Box(Modifier.fillMaxWidth().height(maxOf(2.dp, (ChartHeight - Tokens.Space.S4) * (minutes.toFloat() / peak))).background(colour, ControlShape))
    }
}

@Composable
private fun Legend(name: String, colour: androidx.compose.ui.graphics.Color) {
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.height(Tokens.Size.IconSm).widthIn(min = Tokens.Size.IconSm).background(colour, ControlShape))
        Label(name, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
    }
}

private fun dayName(isoDate: String): String = WeekdayNames[weekdayOf(isoDate)]

@Composable
private fun PomodoroCard(store: Store, wellness: WellnessState) {
    val colors = MyTheme.colors
    val settings = wellness.settings
    val onBreak = wellness.overview?.currentBreak
    var secondsLeft by remember(onBreak?.id) { mutableStateOf(onBreak?.let { secondsUntil(it.endsAt) } ?: 0L) }

    LaunchedEffect(onBreak?.id) {
        while (onBreak != null) {
            secondsLeft = secondsUntil(onBreak.endsAt)
            delay(1000)
        }
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Pomodoro", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
            Checkbox(settings.pomodoro, "Pomodoro", { store.wellness.update { it.copy(pomodoro = !it.pomodoro) } }, checkedState = "On", uncheckedState = "Off")

            Label("Pomodoro")

            when {
                onBreak != null -> Label("On a break, ${countdownText(secondsLeft)} left", style = Tokens.Type.Title)
                settings.pomodoro -> Label("${(PomodoroFocusMinutes - wellness.clock.activeMinutes).coerceAtLeast(0)} min of focus left", style = Tokens.Type.Title)
                else -> Label("25 minutes of focus, then 5 off", color = colors.textMuted)
            }
        }

        if (onBreak != null) {
            MyButton(store.wellness::endBreak, text = "End break", variant = ButtonVariant.Outline)
        } else {
            MyButton(store.wellness::startBreak, text = "Start break", icon = MyIcon.Cup, variant = ButtonVariant.Outline)
        }
    }
}

private fun secondsUntil(iso: String): Long = runCatching { (Instant.parse(iso) - Clock.System.now()).inWholeSeconds }.getOrDefault(0L)

@Composable
private fun PuzzlesCard(store: Store, wellness: WellnessState, platform: Platform) {
    val colors = MyTheme.colors
    val playedToday = wellness.overview?.days?.lastOrNull()?.puzzles.orEmpty()

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Daily puzzles", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        Table(
            listOf(Col("Puzzle", 120.dp, weight = 1f), Col("Open", 96.dp), Col("Played today", 120.dp)),
            Puzzles.size,
            { row, column ->
                val puzzle = Puzzles[row]
                val played = puzzle.id in playedToday

                when (column) {
                    0 -> TableCell.Text(puzzle.name, color = if (played) colors.textMuted else colors.text)
                    1 -> TableCell.Slot("Open", 0.dp) { MyButton({ platform.openUrl(puzzle.url) }, text = "Open", description = "Open ${puzzle.name}", variant = ButtonVariant.Outline, size = ButtonSize.Sm) }
                    else -> TableCell.Slot("Played today", Tokens.Size.ControlMd) { Checkbox(played, "${puzzle.name} played today", { store.wellness.setPlayed(puzzle.id, !played) }, checkedState = "Played", uncheckedState = "Not played") }
                }
            },
            rowKey = { Puzzles[it].id },
            rowDescription = { Puzzles[it].name },
        )
    }
}
