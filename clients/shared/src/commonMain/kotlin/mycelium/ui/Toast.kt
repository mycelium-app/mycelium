package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.state.BreakPhase
import mycelium.state.Store

private val ComposerClearance = 96.dp

enum class ToastTone { Neutral, Error }

@Composable
fun Toast(
    icon: MyIcon,
    iconColor: Color,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    tone: ToastTone = ToastTone.Neutral,
    actions: (@Composable () -> Unit)? = null,
    text: @Composable () -> Unit,
) {
    val colors = MyTheme.colors
    val border = if (tone == ToastTone.Error) colors.statusFailed else colors.borderControl

    Column(
        modifier
            .shadow(Tokens.Space.S2, ControlShape, ambientColor = colors.shadowFloat, spotColor = colors.shadowFloat)
            .background(colors.bgRaised, ControlShape)
            .border(Tokens.Size.Hairline, border, ControlShape)
            .padding(start = Tokens.Space.S3, top = Tokens.Space.S2, end = Tokens.Space.S2, bottom = Tokens.Space.S3)
            .onPreviewKeyEvent { event ->
                if (event.type == KeyEventType.KeyDown && event.key == Key.Escape) {
                    onDismiss()
                    true
                } else {
                    false
                }
            },
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
            Box(Modifier.padding(top = Tokens.Space.S1_5 + Tokens.Space.S0_5)) {
                Icon(icon, iconColor, Tokens.Size.IconMd)
            }

            Column(Modifier.weight(1f).padding(top = Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                text()
            }

            MyButton(onDismiss, icon = MyIcon.Close, description = "Dismiss", muted = true)
        }

        if (actions != null) {
            Row(Modifier.padding(start = Tokens.Size.IconMd + Tokens.Space.S2), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                actions()
            }
        }
    }
}

@Composable
fun ErrorToast(message: String, onDismiss: () -> Unit, modifier: Modifier = Modifier, onRetry: (() -> Unit)? = null) {
    val colors = MyTheme.colors

    Toast(
        MyIcon.Alert,
        colors.statusFailed,
        onDismiss,
        modifier.semantics {
            contentDescription = "Error: $message"
            liveRegion = LiveRegionMode.Assertive
        },
        tone = ToastTone.Error,
        actions = onRetry?.let { retry -> { MyButton(retry, text = "Retry", description = "Retry", variant = ButtonVariant.Outline) } },
    ) {
        Label(message)
    }
}

@Composable
fun ToastHost(store: Store, phone: Boolean, raised: Boolean) {
    val problem by store.problem.collectAsState()
    val retryable by store.retryable.collectAsState()
    val wellness by store.wellness.state.collectAsState()
    val inset = if (phone) Tokens.Space.S2 else Tokens.Space.S4
    val clearance = if (raised) ComposerClearance else 0.dp
    val width = if (phone) Modifier.fillMaxWidth() else Modifier.widthIn(max = Tokens.Size.Menu * 1.4f)

    Box(Modifier.fillMaxSize().padding(start = inset, top = inset, end = inset, bottom = inset + clearance), contentAlignment = if (phone) Alignment.BottomCenter else Alignment.BottomEnd) {
        Column(width, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.Bottom)) {
            problem?.let { message ->
                ErrorToast(message, onDismiss = store::dismissProblem, onRetry = if (retryable != null) store::retryProblem else null)
            }

            if (wellness.clock.phase == BreakPhase.Due) {
                BreakToast(
                    wellness.clock,
                    wellness.clock.breakLength(wellness.settings),
                    onSnooze = store.wellness::snooze,
                    onStartBreak = store.wellness::startBreak,
                    onDismiss = store.wellness::dismiss,
                )
            }
        }
    }
}
