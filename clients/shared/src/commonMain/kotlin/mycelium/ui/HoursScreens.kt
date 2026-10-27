package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.delay
import mycelium.Platform
import mycelium.api.ConversationStatus
import mycelium.api.Group
import mycelium.state.BreakPhase
import mycelium.state.ClosedNotice
import mycelium.state.HoursAction
import mycelium.state.HoursNotice
import mycelium.state.HoursView
import mycelium.state.OpeningMinutes
import mycelium.state.Store
import mycelium.state.clockOf
import mycelium.state.countdownText
import mycelium.state.displayName
import mycelium.state.localTimeOf
import mycelium.state.minutesLeftText
import mycelium.state.stillWorkingOnBreak
import kotlin.time.Clock
import kotlin.time.Duration.Companion.minutes

val LocalHoursView = staticCompositionLocalOf<HoursView?> { null }

internal const val MinuteMillis = 60_000L
internal const val SecondMillis = 1_000L

@Composable
fun rememberHoursView(store: Store): HoursView {
    val wellness by store.wellness.state.collectAsState()
    val state by store.state.collectAsState()
    var now by remember { mutableStateOf(Clock.System.now()) }
    val view = remember(wellness.settings, state.groups, now) { store.hoursView(now) }
    val nextChange = view.groups.values.flatMap { listOfNotNull(it.openedUntil, it.closesAt, it.opensAt) }.filter { it > view.now }.minOrNull()

    LaunchedEffect(Unit) {
        while (true) {
            delay(MinuteMillis)
            now = Clock.System.now()
        }
    }

    LaunchedEffect(nextChange) {
        if (nextChange != null) {
            delay((nextChange - Clock.System.now()).inWholeMilliseconds.coerceAtLeast(0) + 1)
            now = Clock.System.now()
        }
    }

    return view
}

@Composable
fun HoursBanner(notice: HoursNotice, onAction: () -> Unit) {
    val colors = MyTheme.colors

    Row(
        Modifier.fillMaxWidth().background(colors.bgPanel).padding(start = Tokens.Space.S3, end = Tokens.Space.S2),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(MyIcon.Moon, colors.textMuted, Tokens.Size.IconSm)
        Label(notice.message, Modifier.weight(1f).padding(vertical = Tokens.Space.S1_5).semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Meta, colors.textMuted)
        Label("${notice.minutesLeft} min left", Modifier.semantics { contentDescription = "${notice.minutesLeft} ${if (notice.minutesLeft == 1) "minute" else "minutes"} left" }, Tokens.Type.Meta, colors.textMuted, maxLines = 1)
        MyButton(onAction, text = if (notice.action == HoursAction.CloseNow) "Close now" else "Hours", size = ButtonSize.Sm, muted = true)
    }
}

@Composable
private fun CalmScreen(content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(Tokens.Space.S4),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.CenterVertically),
        content = content,
    )
}

@Composable
fun ClosedScreen(notice: ClosedNotice, onSelect: (String) -> Unit, onOpenOutOfHours: () -> Unit) {
    val colors = MyTheme.colors

    CalmScreen {
        Icon(MyIcon.Moon, colors.leaf, Tokens.Size.IconMd * 2)
        Label(notice.title, Modifier.semantics { heading() }, Tokens.Type.Display)
        Label(notice.body, Modifier.widthIn(max = Tokens.Size.FormColumn), color = colors.textMuted)

        if (notice.openNow.isNotEmpty()) {
            Label("Open now", Modifier.padding(top = Tokens.Space.S3), Tokens.Type.MetaStrong, colors.textMuted)

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.CenterHorizontally), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                for (group in notice.openNow) {
                    MyButton({ onSelect(group.id) }, text = displayName(group.name), variant = ButtonVariant.Outline)
                }
            }
        }

        MyButton(onOpenOutOfHours, Modifier.padding(top = Tokens.Space.S4), text = notice.openLabel, muted = true)
    }
}

@Composable
fun BreakScreen(store: Store, platform: Platform?) {
    val wellness by store.wellness.state.collectAsState()
    val state by store.state.collectAsState()
    val now = rememberNow(SecondMillis)
    val colors = MyTheme.colors
    var confirming by remember { mutableStateOf(false) }
    val startedAt = wellness.breakStartedAt
    val endsAt = wellness.breakEndsAt

    LaunchedEffect(now) {
        store.wellness.finishBreakIfOver(now)
    }

    if (wellness.clock.phase != BreakPhase.OnBreak || endsAt == null) {
        return
    }

    val left = endsAt - now
    val total = startedAt?.let { endsAt - it }
    val working = state.conversations.values.count { it.archivedAt == null && it.status == ConversationStatus.Working }

    Box(Modifier.fillMaxSize().background(colors.bgContent).pointerInput(Unit) { detectTapGestures() }.semantics { contentDescription = "Break" }) {
        CalmScreen {
            Icon(MyIcon.Cup, colors.leaf, Tokens.Size.IconMd * 2)
            Label("On a break until ${clockOf(localTimeOf(platform, endsAt))}", Modifier.semantics { heading() }, Tokens.Type.Display)
            Label(countdownText(left), Modifier.semantics { contentDescription = "${minutesLeftText(left)} left" }, Tokens.Type.Display)

            if (total != null && total.isPositive()) {
                DeterminateProgress("Break", 1f - (left / total).toFloat(), Modifier.widthIn(max = Tokens.Size.FormColumn))
            }

            Label("Step away from the screen. Stand up, stretch, and look at something far away.", Modifier.widthIn(max = Tokens.Size.FormColumn), color = colors.textMuted)
            Label(stillWorkingOnBreak(working), Modifier.widthIn(max = Tokens.Size.FormColumn), color = colors.textMuted)
            MyButton({ confirming = true }, Modifier.padding(top = Tokens.Space.S4), text = "End break early", muted = true)
        }
    }

    if (confirming) {
        ConfirmDialog(
            "End your break early?",
            "${minutesLeftText(left)} left. Breaks ended early are counted in Wellness & Learning.",
            "End break early",
            onDismiss = { confirming = false },
            onConfirm = { confirming = false; store.wellness.endBreak() },
        )
    }
}

@Composable
fun OpenOutOfHoursDialog(store: Store, platform: Platform?, closed: List<Group>, initial: String?, onDismiss: () -> Unit, onOpened: (String) -> Unit) {
    val names = closed.map { displayName(it.name) }
    var chosen by remember { mutableStateOf(closed.firstOrNull { it.id == initial } ?: closed.first()) }
    var reason by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val focus = remember { FocusRequester() }
    val runner = rememberAction()
    val closesAt = clockOf(localTimeOf(platform, store.hoursView().now + OpeningMinutes.minutes))

    fun open() {
        if (reason.isBlank()) {
            error = "Enter why you need to open ${displayName(chosen.name)}"
            runCatching { focus.requestFocus() }

            return
        }

        runner {
            store.openGroupOutOfHours(chosen.id, reason.trim())
            onOpened(chosen.id)
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Label("Open a closed group for $OpeningMinutes minutes", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("It closes again at $closesAt. Openings out of hours are counted in Wellness & Learning.", style = Tokens.Type.Meta, color = MyTheme.colors.textMuted)

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Group", style = Tokens.Type.MetaStrong, color = MyTheme.colors.textMuted)
            Segmented(names, displayName(chosen.name), { picked -> chosen = closed[names.indexOf(picked)]; error = null }, groupLabel = "Group")
        }

        TextArea(reason, { reason = it; error = null }, "Why do you need it now?", error = error, errorAbove = true, focus = focus)

        runner.failure?.let { Label(it, style = Tokens.Type.MetaStrong, color = MyTheme.colors.statusFailed) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::open, text = "Open for $OpeningMinutes minutes", variant = ButtonVariant.Primary)
        }
    }
}

class OpenRequest(val initial: String?)

@Composable
fun OpenOutOfHoursHost(store: Store, platform: Platform, hours: HoursView, request: OpenRequest?, onDismiss: () -> Unit, onOpened: (String) -> Unit) {
    val closed = hours.groups.values.filter { !it.open }.map { it.group }.sortedBy { it.name.lowercase() }

    if (request != null && closed.isNotEmpty()) {
        OpenOutOfHoursDialog(store, platform, closed, request.initial, onDismiss, onOpened)
    }
}
