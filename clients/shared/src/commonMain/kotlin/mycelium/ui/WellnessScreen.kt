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
import mycelium.Platform
import mycelium.api.WellnessDay
import mycelium.state.BreakPhase
import mycelium.state.Puzzle
import mycelium.state.Store
import mycelium.state.WellnessState
import mycelium.state.weekdayOf
import mycelium.state.clockOf
import mycelium.state.localTimeOf
import kotlin.time.Instant
import mycelium.state.WeekdayNames

private const val WellnessTabName = "Overview"
private const val SkillsTabName = "Skills"
private const val ReflectionsTabName = "Reflections"
private const val PuzzlesTabName = "Puzzles"
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

            store.wellness.releaseHeld { store.hoursView().closed(it) }.takeIf { it.isNotEmpty() }?.let { released ->
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
fun WellnessScreen(store: Store, platform: Platform, startOn: String = "overview", onOpenAssistant: (String) -> Unit = {}) {
    val wellness by store.wellness.state.collectAsState()
    var tab by remember { mutableStateOf(when (startOn) { "skills" -> SkillsTabName; "puzzles" -> PuzzlesTabName; else -> WellnessTabName }) }
    var playing by remember { mutableStateOf<Puzzle?>(null) }

    LaunchedEffect(Unit) {
        store.wellness.load()
    }

    Column(Modifier.fillMaxSize()) {
        Bar {
            Icon(MyIcon.Leaf, MyTheme.colors.leaf, Tokens.Size.IconMd)
            BarTitle("Wellness & Learning")
        }

        val game = playing

        if (game != null) {
            PuzzleBrowser(store, platform, game) { playing = null }

            return@Column
        }

        TabRow(listOf(WellnessTabName, SkillsTabName, ReflectionsTabName, PuzzlesTabName), tab, { tab = it }, "Wellness and Learning sections", Modifier.padding(horizontal = Tokens.Space.S3))
        Hairline()

        Column(Modifier.weight(1f).fillMaxWidth().verticalScrollWithBar().padding(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S6)) {
            if (tab == WellnessTabName) {
                WellnessOverview(wellness)
                OpenedOutOfHours(wellness)
                LaunchedEffect(Unit) { store.learning.loadSkills() }
                SuggestedExerciseCard(store, Modifier.widthIn(max = Tokens.Size.Page))
                RecentlyLearned(store, { tab = ReflectionsTabName }, Modifier.widthIn(max = Tokens.Size.Page))
                SkillsOverTime(store, { tab = SkillsTabName }, Modifier.widthIn(max = Tokens.Size.Page))
            } else if (tab == SkillsTabName) {
                SkillsTab(store)
            } else if (tab == PuzzlesTabName) {
                PuzzlesTab(store, platform) { playing = it }
            } else {
                ReflectionsTab(store, onOpenAssistant)
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

        wellness.overview?.heldWeek?.let { held ->
            if (held.notifications + held.schedules + held.pings > 0) {
                Label("While closed, Mycelium held ${plural(held.notifications, "notification")}, ${plural(held.schedules, "scheduled run")} and ${plural(held.pings, "assistant ping")} this week.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
            }
        }
    }
}

@Composable
private fun OpenedOutOfHours(wellness: WellnessState) {
    val openings = wellness.overview?.openedOutOfHours.orEmpty()
    val platform = LocalPlatform.current
    val muted = MyTheme.colors.textMuted

    if (openings.isEmpty()) {
        return
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Opened out of hours this week", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        Table(
            listOf(Col("Why", 160.dp, weight = 1f), Col("Group", 80.dp, preferred = 140.dp), Col("When", 110.dp, preferred = 150.dp)),
            openings.size,
            { row, column ->
                val opening = openings[row]
                val local = localTimeOf(platform, Instant.parse(opening.openedAt))

                when (column) {
                    0 -> TableCell.Text(opening.reason)
                    1 -> TableCell.Text(opening.groupName, color = muted)
                    else -> TableCell.Text("${WeekdayNames[weekdayOf(local.substringBefore('T'))]} ${clockOf(local)}", color = muted)
                }
            },
            rowKey = { openings[it].openedAt },
            rowDescription = { openings[it].reason },
        )
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


private fun plural(count: Long, noun: String): String = "$count $noun${if (count == 1L) "" else "s"}"
