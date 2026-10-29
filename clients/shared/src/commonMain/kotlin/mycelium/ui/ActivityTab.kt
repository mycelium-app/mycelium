package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import mycelium.Platform
import mycelium.api.ActivityHours
import mycelium.api.ActivitySpan
import mycelium.state.ActivityKind
import mycelium.state.ActivityTableRow
import mycelium.state.Stretch
import mycelium.state.Store
import mycelium.state.TimelineRange
import mycelium.state.WeekdayNames
import mycelium.state.activityRows
import mycelium.state.addDays
import mycelium.state.clockHour
import mycelium.state.clockMinutes
import mycelium.state.dayHeading
import mycelium.state.heatLevel
import mycelium.state.heatName
import mycelium.state.rangeOf
import mycelium.state.stretchOf
import mycelium.state.stretchesOf
import mycelium.state.weekdayOf

private val ConversationBlue = Color(0xFF3987E5)
private val ChatOrange = Color(0xFFD95926)
private val ChatOrangeDark = Color(0xFFA8421B)
private val AssistantAqua = Color(0xFF199E70)
private val AssistantAquaDark = Color(0xFF0E5C40)
private val ElsewhereGrey = Color(0xFF6B6B6B)
private val ElsewhereGreyDark = Color(0xFF4A4A4A)
private val HeatDark = listOf(Color(0xFF184F95), Color(0xFF256ABF), Color(0xFF3987E5), Color(0xFF86B6EF))
private val HeatLight = listOf(Color(0xFFC7DCF7), Color(0xFF92BBEE), Color(0xFF5B97E3), Color(0xFF1F58A8))
private val HeatLightEdge = Color(0xFF1F58A8)

private val LaneLabelWidth = 64.dp
private val LaneHeight = 28.dp
private val BreakLaneHeight = 20.dp
private val PhoneLaneHeight = 32.dp
private val MinimumMark = 6.dp
private val LabelRoom = 56.dp
private val SwatchSize = 14.dp
private val HeatCellSize = 22.dp
private val HeatGap = 2.dp
private val HeatDayLabelWidth = 40.dp
private val PhoneHourLabelWidth = 36.dp
private val PhoneHeatRow = 24.dp
private val AxisTick = 36.dp

private enum class Fill { Conversation, Chat, Assistant, Elsewhere, Break, Away }

private fun ActivityKind.fill(): Fill = when (this) {
    ActivityKind.Conversation -> Fill.Conversation
    ActivityKind.Chat -> Fill.Chat
    ActivityKind.Assistant -> Fill.Assistant
    ActivityKind.Elsewhere -> Fill.Elsewhere
}

private data class Mark(
    val fill: Fill,
    val startMinute: Int,
    val endMinute: Int,
    val label: String?,
    val description: String,
    val tip: List<String>,
)

private fun DrawScope.stripes(spacing: Float, width: Float, color: Color, rising: Boolean) {
    val height = size.height
    var x = -height

    while (x < size.width) {
        if (rising) {
            drawLine(color, Offset(x, height), Offset(x + height, 0f), width)
        } else {
            drawLine(color, Offset(x, 0f), Offset(x + height, height), width)
        }

        x += spacing
    }
}

private fun DrawScope.dots(spacing: Float, radius: Float, color: Color) {
    var y = spacing / 2

    while (y < size.height) {
        var x = spacing / 2

        while (x < size.width) {
            drawCircle(color, radius, Offset(x, y))
            x += spacing
        }

        y += spacing
    }
}

@Composable
private fun Modifier.filled(fill: Fill, shape: Shape): Modifier {
    val leaf = MyTheme.colors.leaf
    val edge = MyTheme.colors.borderControl
    val diagonal = 1.4142f

    return when (fill) {
        Fill.Conversation -> background(ConversationBlue, shape)
        Fill.Chat -> clip(shape).drawBehind {
            drawRect(ChatOrange)
            stripes(6.dp.toPx() * diagonal, 2.dp.toPx(), ChatOrangeDark, rising = true)
        }
        Fill.Assistant -> clip(shape).drawBehind {
            drawRect(AssistantAqua)
            dots(5.dp.toPx(), 1.2.dp.toPx(), AssistantAquaDark)
        }
        Fill.Elsewhere -> clip(shape).drawBehind {
            drawRect(ElsewhereGrey)
            stripes(6.dp.toPx() * diagonal, 2.dp.toPx(), ElsewhereGreyDark, rising = false)
        }
        Fill.Break -> clip(shape).drawBehind {
            drawRect(leaf.copy(alpha = 0.25f))
            stripes(5.dp.toPx() * diagonal, 2.dp.toPx(), leaf.copy(alpha = 0.7f), rising = true)
            drawRoundRect(leaf, cornerRadius = CornerRadius(2.dp.toPx()), style = Stroke(1.dp.toPx()))
        }
        Fill.Away -> drawBehind {
            drawRoundRect(edge, cornerRadius = CornerRadius(4.dp.toPx()), style = Stroke(1.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(4.dp.toPx(), 3.dp.toPx()))))
        }
    }
}

@Composable
private fun heatColor(level: Int): Color {
    val colors = MyTheme.colors

    return when {
        level <= 0 -> colors.bgRaised
        LocalAppearance.current.dark -> HeatDark[level - 1]
        else -> HeatLight[level - 1]
    }
}

@Composable
private fun Modifier.heatFilled(level: Int, shape: Shape): Modifier {
    val colors = MyTheme.colors
    val dark = LocalAppearance.current.dark
    val edge = when {
        level <= 0 -> colors.border
        dark -> Color.Transparent
        else -> HeatLightEdge
    }

    return background(heatColor(level), shape).border(Tokens.Size.Hairline, edge, shape)
}

@Composable
private fun Swatch(fill: Fill) {
    Box(Modifier.size(SwatchSize).filled(fill, ControlShape))
}

@Composable
private fun HeatSwatch(level: Int) {
    Box(Modifier.size(SwatchSize).heatFilled(level, ControlShape))
}

@Stable
private class Roving(count: Int) {
    val requesters = List(count) { FocusRequester() }
    var active by mutableIntStateOf(0)
    var moves by mutableIntStateOf(0)

    fun move(to: Int) {
        if (requesters.isEmpty()) {
            return
        }

        active = to.coerceIn(0, requesters.lastIndex)
        moves += 1
    }
}

@Composable
private fun rememberRoving(vararg keys: Any?, count: Int): Roving {
    val roving = remember(*keys, count) { Roving(count) }

    LaunchedEffect(roving, roving.moves) {
        if (roving.moves > 0) {
            runCatching { roving.requesters[roving.active].requestFocus() }
        }
    }

    return roving
}

private fun Modifier.rovingStop(roving: Roving, index: Int, target: (Key) -> Int?): Modifier =
    focusRequester(roving.requesters[index])
        .focusProperties { canFocus = roving.active == index }
        .onKeyEvent { event ->
            val to = if (event.type == KeyEventType.KeyDown) target(event.key) else null

            if (to == null) {
                false
            } else {
                roving.move(to)
                true
            }
        }

@Composable
private fun Tip(lines: List<String>) {
    val colors = MyTheme.colors
    val gap = with(LocalDensity.current) { Tokens.Space.S1.roundToPx() }

    Popup(popupPositionProvider = remember(gap) { BelowAnchor(gap, centred = true) }) {
        Column(
            Modifier
                .background(colors.bgRaised, FloatShape)
                .border(Tokens.Size.Hairline, colors.border, FloatShape)
                .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1_5),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
        ) {
            lines.forEachIndexed { position, line ->
                Label(line, style = if (position == 0) Tokens.Type.MetaStrong else Tokens.Type.Meta, color = if (position == 0) colors.text else colors.textMuted)
            }
        }
    }
}

private fun spanDuration(minutes: Int): String = if (minutes < 1) "Under 1 min" else durationText(minutes.toLong())

private fun stretchName(span: ActivitySpan): String = span.project?.let { "$it ${span.title}" } ?: span.title

private fun kindSentence(kind: ActivityKind, group: String?): String = when {
    kind == ActivityKind.Elsewhere -> kind.noun
    group != null -> "${kind.noun} in $group"
    else -> kind.noun
}

private fun timesText(stretch: Stretch): String = "${clockMinutes(stretch.startMinute)} to ${clockMinutes(stretch.endMinute)}, ${spanDuration(stretch.minutes)}"

private fun markOf(stretch: Stretch, range: TimelineRange, phone: Boolean): Mark {
    val span = stretch.span
    val name = stretchName(span)
    val kind = kindSentence(stretch.kind, span.group)
    val room = stretch.minutes.toFloat() / range.minutes

    return Mark(
        fill = stretch.kind.fill(),
        startMinute = stretch.startMinute,
        endMinute = stretch.endMinute,
        label = if (phone || stretch.kind == ActivityKind.Elsewhere || room < 0.07f) null else span.project ?: span.title,
        description = "$name. $kind. ${timesText(stretch)}",
        tip = listOf(name, kind, timesText(stretch)),
    )
}

private fun breakMark(stretch: Stretch, range: TimelineRange): Mark? {
    val start = stretch.startMinute.coerceIn(range.startMinute, range.startMinute + range.minutes)
    val end = stretch.endMinute.coerceIn(range.startMinute, range.startMinute + range.minutes)

    if (end <= start) {
        return null
    }

    val times = "${clockMinutes(start)} to ${clockMinutes(end)}, ${spanDuration(end - start)}"

    return Mark(Fill.Break, start, end, null, "Break, $times", listOf("Break", times))
}

@Composable
private fun MarkButton(mark: Mark, index: Int, roving: Roving, last: Int, phone: Boolean, selected: Boolean, onSelect: () -> Unit, modifier: Modifier) {
    val colors = MyTheme.colors
    val source = remember { MutableInteractionSource() }
    val focused by source.collectIsFocusedAsState()
    val hovered by source.collectIsHoveredAsState()

    LaunchedEffect(focused) {
        if (focused) {
            roving.active = index
        }
    }

    Box(
        modifier
            .rovingStop(roving, index) { key ->
                when (key) {
                    Key.DirectionLeft -> (index - 1).takeIf { it >= 0 }
                    Key.DirectionRight -> (index + 1).takeIf { it <= last }
                    Key.MoveHome -> 0
                    Key.MoveEnd -> last
                    else -> null
                }
            }
            .pressable(onSelect, description = mark.description, shape = SegmentShape, source = source)
            .filled(mark.fill, SegmentShape)
            .then(if (selected) Modifier.border(Tokens.Space.S0_5, colors.accent, SegmentShape) else Modifier),
    ) {
        if (mark.label != null) {
            Label(
                mark.label,
                Modifier
                    .align(Alignment.CenterStart)
                    .padding(start = Tokens.Space.S1_5)
                    .background(colors.bgContent.copy(alpha = 0.85f), SegmentShape)
                    .padding(horizontal = Tokens.Space.S1)
                    .clearAndSetSemantics {},
                Tokens.Type.MetaStrong,
                maxLines = 1,
            )
        }

        if (!phone && (focused || hovered)) {
            Tip(mark.tip)
        }
    }
}

@Composable
private fun Lane(marks: List<Mark>, range: TimelineRange, height: Dp, phone: Boolean, selected: Int?, onSelect: (Int) -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val roving = rememberRoving(marks.map { it.description }, count = marks.size)

    BoxWithConstraints(modifier.fillMaxWidth().height(height)) {
        Box(Modifier.align(Alignment.Center).fillMaxWidth().height(Tokens.Size.Hairline).background(colors.border))

        marks.forEachIndexed { index, mark ->
            val left = maxWidth * range.fraction(mark.startMinute)
            val width = maxOf(maxWidth * (range.fraction(mark.endMinute) - range.fraction(mark.startMinute)) - Tokens.Space.S0_5, MinimumMark)

            MarkButton(mark, index, roving, marks.lastIndex, phone, selected == index, { onSelect(index) }, Modifier.offset(x = left).width(width).fillMaxHeight())
        }
    }
}

@Composable
private fun Axis(range: TimelineRange, phone: Boolean) {
    val hours = range.endHour - range.startHour
    val every = if (phone) maxOf(1, (hours + 3) / 4) else maxOf(1, (hours + 7) / 8)
    val colors = MyTheme.colors

    BoxWithConstraints(Modifier.fillMaxWidth().height(Tokens.Space.S4).clearAndSetSemantics {}) {
        for (hour in range.startHour..range.endHour step every) {
            val centre = maxWidth * range.fraction(hour * 60)
            val left = (centre - AxisTick / 2).coerceIn(0.dp, maxOf(0.dp, maxWidth - AxisTick))

            Box(Modifier.offset(x = left).width(AxisTick), contentAlignment = Alignment.Center) {
                Label(clockHour(hour % 24), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }
    }
}

@Composable
private fun Legend(names: List<String>, swatch: @Composable (Int) -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S4), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
        names.forEachIndexed { position, name ->
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                swatch(position)
                Label(name, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)
            }
        }
    }
}

@Composable
private fun TimelineSection(stretches: List<Stretch>, breaks: List<Stretch>, range: TimelineRange, phone: Boolean) {
    val colors = MyTheme.colors
    val onScreen = stretches.map { markOf(it, range, phone) }
    val breakMarks = breaks.mapNotNull { breakMark(it, range) }
    var picked by remember(onScreen.map { it.description }) { mutableStateOf<Mark?>(null) }
    val hint = if (phone) "Tap a stretch for its details; gaps are away." else "Left and Right arrows move between stretches; gaps are away."

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Day timeline", style = Tokens.Type.Meta, color = colors.textMuted)

        Column(Modifier.semantics { contentDescription = "Day timeline. $hint" }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label("On screen", Modifier.width(LaneLabelWidth), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                Lane(onScreen, range, if (phone) PhoneLaneHeight else LaneHeight, phone, onScreen.indexOf(picked).takeIf { it >= 0 }, { picked = onScreen[it] }, Modifier.weight(1f))
            }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                Label("Breaks", Modifier.width(LaneLabelWidth), Tokens.Type.Meta, colors.textMuted, maxLines = 1)
                Lane(breakMarks, range, BreakLaneHeight, phone, null, {}, Modifier.weight(1f))
            }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                Spacer(Modifier.width(LaneLabelWidth))
                Box(Modifier.weight(1f)) { Axis(range, phone) }
            }
        }

        val details = picked

        if (phone && details != null) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .border(Tokens.Size.Hairline, colors.border, ControlShape)
                    .background(colors.bgPanel, ControlShape)
                    .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)
                    .semantics { liveRegion = LiveRegionMode.Polite },
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
            ) {
                details.tip.forEachIndexed { position, line ->
                    Label(line, style = if (position == 0) Tokens.Type.MetaStrong else Tokens.Type.Meta, color = if (position == 1) colors.textMuted else colors.text)
                }
            }
        }

        Legend(ActivityKind.entries.map { it.legend } + listOf("Break", "Away")) { position -> Swatch(LegendFills[position]) }
    }
}

private val LegendFills = ActivityKind.entries.map { it.fill() } + listOf(Fill.Break, Fill.Away)

private val HeatLegend = listOf("None", "Under 15 min", "15 to 30 min", "30 to 45 min", "45 min or more")

@Composable
private fun HeatLegendRow() {
    Legend(HeatLegend) { level -> HeatSwatch(level) }
}

@Composable
private fun HeatCell(
    level: Int,
    name: String,
    tip: List<String>,
    index: Int,
    roving: Roving,
    keys: (Key) -> Int?,
    onActivate: () -> Unit,
    phone: Boolean,
    modifier: Modifier,
) {
    val source = remember { MutableInteractionSource() }
    val focused by source.collectIsFocusedAsState()
    val hovered by source.collectIsHoveredAsState()

    LaunchedEffect(focused) {
        if (focused) {
            roving.active = index
        }
    }

    Box(
        modifier
            .rovingStop(roving, index, keys)
            .pressable(onActivate, description = name, shape = SegmentShape, source = source)
            .heatFilled(level, SegmentShape),
    ) {
        if (!phone && (focused || hovered)) {
            Tip(tip)
        }
    }
}

@Composable
private fun DayLabel(date: String, on: Boolean, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val weekday = WeekdayNames[weekdayOf(date)]

    Row(
        modifier.semantics { contentDescription = if (on) "$weekday, shown in the timeline" else weekday; selected = on },
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (on) {
            Box(Modifier.width(Tokens.Space.S0_5).height(12.dp).background(colors.text))
        }

        Label(weekday.take(3), style = if (on) Tokens.Type.MetaStrong else Tokens.Type.Meta, color = if (on) colors.text else colors.textMuted, maxLines = 1, modifier = Modifier.clearAndSetSemantics {})
    }
}

@Composable
private fun Heatmap(hours: ActivityHours, shown: String?, phone: Boolean, onShow: (String) -> Unit) {
    val colors = MyTheme.colors
    val days = hours.days
    val roving = rememberRoving(days.map { it.date }, count = days.size * 24)
    var picked by remember(days.map { it.date }) { mutableStateOf<Int?>(null) }

    fun minutesAt(index: Int): Long = days[index / 24].minutes.getOrElse(index % 24) { 0 }

    fun nameAt(index: Int): String = heatName(days[index / 24].date, index % 24, minutesAt(index))

    fun tipAt(index: Int): List<String> {
        val day = days[index / 24]
        val top = day.top.getOrNull(index % 24)

        return listOfNotNull(nameAt(index), top?.let { "Mostly $it" }, "Enter to show ${WeekdayNames[weekdayOf(day.date)]}")
    }

    fun keysFor(index: Int): (Key) -> Int? = { key ->
        val hour = index % 24
        val day = index / 24
        val hourStep = if (phone) Key.DirectionDown to Key.DirectionUp else Key.DirectionRight to Key.DirectionLeft
        val dayStep = if (phone) Key.DirectionRight to Key.DirectionLeft else Key.DirectionDown to Key.DirectionUp

        when (key) {
            hourStep.first -> (index + 1).takeIf { hour < 23 }
            hourStep.second -> (index - 1).takeIf { hour > 0 }
            dayStep.first -> (index + 24).takeIf { day < days.lastIndex }
            dayStep.second -> (index - 24).takeIf { day > 0 }
            Key.MoveHome -> day * 24
            Key.MoveEnd -> day * 24 + 23
            else -> null
        }
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("Last 7 days by hour", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        if (phone) {
            HeatLegendRow()
        }

        Column(Modifier.semantics { contentDescription = "Time on screen, last 7 days by hour. Arrow keys move between hours and days; Enter shows that day in the timeline." }) {
            if (phone) {
                Row(horizontalArrangement = Arrangement.spacedBy(HeatGap)) {
                    Spacer(Modifier.width(PhoneHourLabelWidth))

                    days.forEach { day ->
                        DayLabel(day.date, day.date == shown, Modifier.weight(1f))
                    }
                }

                Column(verticalArrangement = Arrangement.spacedBy(HeatGap)) {
                    for (hour in 0 until 24) {
                        Row(horizontalArrangement = Arrangement.spacedBy(HeatGap), verticalAlignment = Alignment.CenterVertically) {
                            Label(hour.toString().padStart(2, '0'), Modifier.width(PhoneHourLabelWidth).clearAndSetSemantics {}, Tokens.Type.Meta, colors.textMuted, maxLines = 1)

                            days.forEachIndexed { dayIndex, _ ->
                                val index = dayIndex * 24 + hour

                                HeatCell(heatLevel(minutesAt(index)), nameAt(index), tipAt(index), index, roving, keysFor(index), { picked = index }, true, Modifier.weight(1f).height(PhoneHeatRow))
                            }
                        }
                    }
                }
            } else {
                BoxWithConstraints(Modifier.fillMaxWidth()) {
                    val cell = minOf(HeatCellSize, (maxWidth - HeatDayLabelWidth - HeatGap * 24) / 24)

                    Column(verticalArrangement = Arrangement.spacedBy(HeatGap)) {
                        Row(Modifier.clearAndSetSemantics {}) {
                            Spacer(Modifier.width(HeatDayLabelWidth + HeatGap))

                            for (hour in 0 until 24 step 3) {
                                Box(Modifier.width((cell + HeatGap) * 3)) {
                                    Label(hour.toString().padStart(2, '0'), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                                }
                            }
                        }

                        days.forEachIndexed { dayIndex, day ->
                            Row(horizontalArrangement = Arrangement.spacedBy(HeatGap), verticalAlignment = Alignment.CenterVertically) {
                                DayLabel(day.date, day.date == shown, Modifier.width(HeatDayLabelWidth))

                                for (hour in 0 until 24) {
                                    val index = dayIndex * 24 + hour

                                    HeatCell(heatLevel(minutesAt(index)), nameAt(index), tipAt(index), index, roving, keysFor(index), { onShow(day.date) }, false, Modifier.size(cell))
                                }
                            }
                        }
                    }
                }
            }
        }

        if (!phone) {
            HeatLegendRow()
        }

        val index = picked

        if (phone && index != null) {
            val date = days[index / 24].date

            Column(
                Modifier
                    .fillMaxWidth()
                    .border(Tokens.Size.Hairline, colors.border, ControlShape)
                    .background(colors.bgPanel, ControlShape)
                    .padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2)
                    .semantics { liveRegion = LiveRegionMode.Polite },
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5),
            ) {
                Label(nameAt(index), style = Tokens.Type.MetaStrong)
                days[index / 24].top.getOrNull(index % 24)?.let { Label("Mostly $it", style = Tokens.Type.Meta, color = colors.textMuted) }
                MyButton({ onShow(date) }, text = "Show this day", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            }
        }
    }
}

@Composable
private fun DayHeader(date: String, minutes: Long?, onToday: Boolean, phone: Boolean, onStep: (Int) -> Unit, onTodayClick: () -> Unit) {
    val colors = MyTheme.colors
    val total = minutes?.takeIf { it > 0 }?.let { "${durationText(it)} on screen" }
    val title = @Composable { Label(dayHeading(date), Modifier.semantics { heading(); liveRegion = LiveRegionMode.Polite }, Tokens.Type.UiStrong) }

    Row(horizontalArrangement = Arrangement.spacedBy(if (phone) Tokens.Space.S1 else Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
        MyButton({ onStep(-1) }, icon = MyIcon.ChevronLeft, description = "Previous day, ${dayHeading(addDays(date, -1))}", muted = true)

        if (phone) {
            Column {
                title()
                total?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted) }
            }
        } else {
            title()
        }

        if (onToday) {
            Spacer(Modifier.width(touchMin(Tokens.Size.ControlMd)))
        } else {
            MyButton({ onStep(1) }, icon = MyIcon.ChevronRight, description = "Next day, ${dayHeading(addDays(date, 1))}", muted = true)
        }

        if (!phone) {
            total?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted) }
        }

        Spacer(Modifier.weight(1f))

        if (!onToday) {
            MyButton(onTodayClick, text = "Today", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }
    }
}

@Composable
private fun EmptyDay(date: String, today: Boolean, recordedSince: String?) {
    val colors = MyTheme.colors
    val beforeRecording = recordedSince == null || date < recordedSince
    val reason = when {
        today -> "Time on screen shows here once Mycelium has been open for a minute."
        recordedSince != null && beforeRecording -> "Mycelium began recording the time of day on ${dayHeading(recordedSince)}. Earlier days only have the totals on Overview."
        else -> "Mycelium was not open that day. Agents may still have worked; their time is under With agents on Overview."
    }

    Column(
        Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.border, ControlShape).padding(Tokens.Space.S4),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Icon(MyIcon.Clock, colors.textMuted)
            Label("Nothing on screen on ${dayHeading(date)}", style = Tokens.Type.UiStrong)
        }

        Label(reason, color = colors.textMuted)
    }
}

@Composable
private fun OnScreenTable(rows: List<ActivityTableRow>, onOpenConversation: (String) -> Unit, onOpenAssistant: (String) -> Unit) {
    val colors = MyTheme.colors

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
        Label("What was on screen", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        Table(
            listOf(Col("What", 160.dp, weight = 1f), Col("Kind", 90.dp, preferred = 170.dp), Col("Group", 60.dp, preferred = 90.dp), Col("Time", 70.dp, preferred = 100.dp, numeric = true)),
            rows.size,
            { index, column ->
                val row = rows[index]
                val name = row.project?.let { "$it ${row.title}" } ?: row.title
                val open: (() -> Unit)? = when {
                    row.conversationId != null -> ({ onOpenConversation(row.conversationId) })
                    row.assistantId != null -> ({ onOpenAssistant(row.assistantId) })
                    else -> null
                }

                when (column) {
                    0 -> TableCell.Slot(measureAs = name, extra = SwatchSize + Tokens.Space.S2) {
                        Row(
                            if (open != null) Modifier.pressable(open, description = "Open $name", shape = ControlShape) else Modifier,
                            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Swatch(row.kind.fill())
                            Label(name, color = colors.text, maxLines = 1)
                        }
                    }
                    1 -> TableCell.Text(row.kind.noun, color = colors.textMuted)
                    2 -> TableCell.Text(row.group.orEmpty(), color = colors.textMuted)
                    else -> TableCell.Text(spanDuration(row.minutes.toInt()), color = colors.textMuted)
                }
            },
            rowKey = { index -> "${rows[index].kind}:${rows[index].title}" },
            rowDescription = { rows[it].title },
        )
    }
}

@Composable
fun ActivityTab(store: Store, platform: Platform, onOpenConversation: (String) -> Unit, onOpenAssistant: (String) -> Unit) {
    val wellness by store.wellness.state.collectAsState()
    val view = wellness.activity
    val phone = LocalTouch.current
    val date = view.date
    val day = view.day

    LaunchedEffect(Unit) {
        store.wellness.loadActivity()
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(if (phone) Tokens.Space.S4 else Tokens.Space.S6)) {
        if (date == null) {
            if (view.failed) {
                LoadFailedState("the time of day", onRetry = { store.wellness.reload() })
            }

            return@Column
        }

        DayHeader(date, day?.onScreenMinutes, view.onToday, phone, store.wellness::stepActivityDay, { view.today?.let(store.wellness::showActivityDay) })

        if (day != null) {
            val stretches = stretchesOf(day.spans, platform)
            val range = rangeOf(stretches)

            if (range == null) {
                EmptyDay(date, view.onToday, day.recordedSince)
            } else {
                val breaks = day.breaks.mapNotNull { stretchOf(ActivitySpan(it.start, it.end, ActivityKind.Elsewhere.wire, "Break"), platform) }

                TimelineSection(stretches, breaks, range, phone)
            }
        } else if (view.failed) {
            LoadFailedState("this day", onRetry = { store.wellness.reload() })
        }

        view.hours?.let { hours ->
            Heatmap(hours, date, phone) { store.wellness.showActivityDay(it) }
        }

        if (day != null && day.spans.isNotEmpty()) {
            OnScreenTable(activityRows(day.spans), onOpenConversation, onOpenAssistant)
        }
    }
}
