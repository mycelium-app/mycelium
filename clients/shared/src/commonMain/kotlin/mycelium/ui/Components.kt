package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import mycelium.api.ConversationStatus

val ControlShape = RoundedCornerShape(Tokens.Radius.Control)
val BlockShape = RoundedCornerShape(Tokens.Radius.Block)
val FloatShape = RoundedCornerShape(Tokens.Radius.Float)
val SegmentShape = RoundedCornerShape(Tokens.Radius.Control - Tokens.Space.S0_5)

@Composable
fun touchMin(base: Dp): Dp = if (LocalTouch.current) maxOf(base, Tokens.Size.Touch) else base

@Composable
fun rowHeight(): Dp = touchMin(Tokens.Size.Row)

@Composable
fun barHeight(): Dp = if (LocalTouch.current) Tokens.Size.BarTouch else Tokens.Size.Bar

@Composable
fun Label(
    text: String,
    modifier: Modifier = Modifier,
    style: TextStyle = Tokens.Type.Ui,
    color: Color = MyTheme.colors.text,
    maxLines: Int = Int.MAX_VALUE,
    overflow: TextOverflow = if (maxLines == 1) TextOverflow.Ellipsis else TextOverflow.Clip,
) {
    BasicText(text, modifier, style.themed().copy(color = color), maxLines = maxLines, overflow = overflow)
}

@Composable
fun Hairline(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxWidth().height(Tokens.Size.Hairline).background(MyTheme.colors.border))
}

@Composable
fun VerticalHairline(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxHeight().width(Tokens.Size.Hairline).background(MyTheme.colors.border))
}

@Composable
fun Modifier.focusRing(focused: State<Boolean>, shape: Shape = RectangleShape, inside: Boolean = false, always: Boolean = false): Modifier {
    val ring = MyTheme.colors.focus
    val inputMode = LocalInputModeManager.current

    return drawWithContent {
        drawContent()

        if (focused.value && (always || inputMode.inputMode == InputMode.Keyboard)) {
            val stroke = Tokens.Size.FocusRing.toPx()
            val grow = if (inside) -stroke / 2 else Tokens.Size.FocusOffset.toPx() + stroke / 2
            val corner = if (shape is RoundedCornerShape) shape.topStart.toPx(size, this) else 0f
            val radius = (corner + grow).coerceAtLeast(0f)

            drawRoundRect(
                color = ring,
                topLeft = Offset(-grow, -grow),
                size = Size(size.width + grow * 2, size.height + grow * 2),
                cornerRadius = CornerRadius(radius),
                style = Stroke(stroke),
            )
        }
    }
}

@Composable
fun Modifier.pressable(
    onClick: () -> Unit,
    role: Role? = Role.Button,
    description: String? = null,
    shape: Shape = RectangleShape,
    base: Color = Color.Transparent,
    selected: Boolean = false,
    stateDescription: String? = null,
    ringInside: Boolean = false,
    source: MutableInteractionSource = remember { MutableInteractionSource() },
): Modifier {
    val hovered = source.collectIsHoveredAsState()
    val focused = source.collectIsFocusedAsState()
    val colors = MyTheme.colors

    return this
        .focusRing(focused, shape, ringInside)
        .clip(shape)
        .drawBehind {
            val background = when {
                selected -> colors.bgSelected
                hovered.value && base == Color.Transparent -> colors.bgHover
                hovered.value -> lerp(base, colors.text, 0.12f)
                else -> base
            }

            drawRect(background)
        }
        .hoverable(source)
        .clickable(interactionSource = source, indication = null, role = role, onClick = onClick)
        .semantics {
            if (description != null) {
                contentDescription = description
            }

            if (stateDescription != null) {
                this.stateDescription = stateDescription
            }
        }
}

enum class ButtonVariant { Ghost, Outline, Primary, Danger }

enum class ButtonSize { Sm, Md, Lg }

@Composable
fun MyButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    text: String? = null,
    icon: MyIcon? = null,
    description: String? = null,
    variant: ButtonVariant = ButtonVariant.Ghost,
    size: ButtonSize = ButtonSize.Md,
    muted: Boolean = false,
    selected: Boolean = false,
    trailingIcon: MyIcon? = null,
    tooltip: String? = if (text == null) description else null,
) {
    val colors = MyTheme.colors
    val source = remember { MutableInteractionSource() }
    val height = touchMin(
        when (size) {
            ButtonSize.Sm -> Tokens.Size.ControlSm
            ButtonSize.Md -> Tokens.Size.ControlMd
            ButtonSize.Lg -> Tokens.Size.ControlLg
        },
    )
    val padding = when (size) {
        ButtonSize.Sm -> Tokens.Space.S1_5
        ButtonSize.Md -> Tokens.Space.S2
        ButtonSize.Lg -> Tokens.Space.S3
    }
    val gap = if (size == ButtonSize.Sm) Tokens.Space.S1 else Tokens.Space.S1_5
    val iconSize = if (size == ButtonSize.Sm) Tokens.Size.IconSm else Tokens.Size.IconMd
    val contentColor = when {
        variant == ButtonVariant.Primary -> colors.onAccent
        variant == ButtonVariant.Danger -> colors.statusFailed
        muted -> colors.textMuted
        else -> colors.text
    }
    val textStyle = (if (size == ButtonSize.Sm) Tokens.Type.Meta else Tokens.Type.Ui).let {
        if (variant == ButtonVariant.Primary) it.copy(fontWeight = Tokens.Type.UiStrong.fontWeight) else it
    }
    val sized = if (text == null) modifier.size(height) else modifier.height(height)
    val outlined = if (variant == ButtonVariant.Outline) sized.border(Tokens.Size.Hairline, colors.borderControl, ControlShape) else sized

    Box {
        Row(
            outlined
                .pressable(
                    onClick = onClick,
                    description = description,
                    shape = ControlShape,
                    base = if (variant == ButtonVariant.Primary) colors.accent else Color.Transparent,
                    selected = selected,
                    source = source,
                )
                .padding(horizontal = if (text == null) 0.dp else padding),
            horizontalArrangement = Arrangement.spacedBy(gap, Alignment.CenterHorizontally),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (icon != null) {
                Icon(icon, contentColor, iconSize)
            }

            if (text != null) {
                Label(text, style = textStyle, color = contentColor, maxLines = 1)
            }

            if (trailingIcon != null) {
                Icon(trailingIcon, contentColor, iconSize)
            }
        }

        if (tooltip != null) {
            Tooltip(tooltip, source)
        }
    }
}

@Composable
fun Bar(
    modifier: Modifier = Modifier,
    chrome: Boolean = false,
    startPadding: Dp = Tokens.Space.S3,
    content: @Composable RowScope.() -> Unit,
) {
    val colors = MyTheme.colors

    Column(modifier.background(if (chrome) colors.bgChrome else colors.bgContent)) {
        Row(
            Modifier.fillMaxWidth().height(barHeight()).padding(start = startPadding, end = Tokens.Space.S2),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
            verticalAlignment = Alignment.CenterVertically,
            content = content,
        )
        Hairline()
    }
}

@Composable
fun BarTitle(text: String, modifier: Modifier = Modifier) {
    LocalOpenList.current?.let { openList ->
        MyButton(openList, icon = MyIcon.Menu, description = "Conversations", muted = true)
    }

    Label(text, modifier.semantics { heading() }, Tokens.Type.Title, maxLines = 1)
}

@Composable
fun StatusGlyph(status: ConversationStatus, size: Dp = Tokens.Size.IconSm) {
    val colors = MyTheme.colors
    val (icon, tint) = when (status) {
        ConversationStatus.Input -> MyIcon.Alert to colors.statusAttention
        ConversationStatus.Failed -> MyIcon.Failed to colors.statusFailed
        ConversationStatus.Review -> MyIcon.Review to colors.statusAttention
        ConversationStatus.Working -> MyIcon.Working to colors.statusWorking
        ConversationStatus.Done -> MyIcon.Check to colors.textMuted
    }

    Icon(icon, tint, size)
}

enum class Tone { Working, Attention, Failed, Muted }

@Composable
fun StatusMark(text: String, tone: Tone, icon: MyIcon, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val markColor = when (tone) {
        Tone.Working -> colors.statusWorking
        Tone.Attention -> colors.statusAttention
        Tone.Failed -> colors.statusFailed
        Tone.Muted -> colors.textMuted
    }
    val wordColor = if (tone == Tone.Failed) colors.statusFailed else colors.textMuted

    Row(modifier, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, markColor, Tokens.Size.IconSm)
        Label(text, style = Tokens.Type.Meta, color = wordColor, maxLines = 1)
    }
}

@Composable
fun Dot(color: Color, hollow: Boolean = false) {
    val shape = RoundedCornerShape(Tokens.Radius.Full)
    val base = Modifier.size(Tokens.Size.Dot)

    Box(if (hollow) base.border(Tokens.Size.Hairline, color, shape) else base.background(color, shape))
}

@Composable
fun Tag(text: String) {
    Box(
        Modifier
            .height(Tokens.Size.ControlSm)
            .border(Tokens.Size.Hairline, MyTheme.colors.border, ControlShape)
            .padding(horizontal = Tokens.Space.S1_5),
        contentAlignment = Alignment.Center,
    ) {
        Label(text, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted, maxLines = 1)
    }
}

@Composable
fun TextInput(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    placeholder: String = "",
    error: String? = null,
    mono: Boolean = false,
    large: Boolean = false,
    showLabel: Boolean = true,
    onSubmit: (() -> Unit)? = null,
    masked: Boolean = false,
) {
    val colors = MyTheme.colors
    val source = remember { MutableInteractionSource() }
    val focused = source.collectIsFocusedAsState()
    val height = touchMin(if (large) Tokens.Size.ControlLg else Tokens.Size.ControlMd)
    val borderColor = if (error != null) colors.statusFailed else colors.borderControl
    val base = if (mono) Tokens.Type.Code else Tokens.Type.Ui

    Column(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        if (showLabel) {
            Label(label, style = Tokens.Type.MetaStrong, color = colors.textMuted)
        }

        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            singleLine = true,
            visualTransformation = if (masked) PasswordVisualTransformation() else VisualTransformation.None,
            textStyle = base.themed().copy(color = colors.text),
            cursorBrush = SolidColor(colors.text),
            interactionSource = source,
            keyboardOptions = KeyboardOptions(imeAction = androidx.compose.ui.text.input.ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { onSubmit?.invoke() }),
            modifier = Modifier
                .fillMaxWidth()
                .height(height)
                .focusRing(focused, ControlShape, always = true)
                .background(colors.bgContent, ControlShape)
                .border(Tokens.Size.Hairline, borderColor, ControlShape)
                .semantics {
                    contentDescription = if (error != null) "$label. Error: $error" else label
                },
            decorationBox = { inner ->
                Box(Modifier.padding(horizontal = Tokens.Space.S2), contentAlignment = Alignment.CenterStart) {
                    if (value.isEmpty() && placeholder.isNotEmpty()) {
                        Label(placeholder, style = base, color = colors.textFaint, maxLines = 1)
                    }

                    inner()
                }
            },
        )

        if (error != null) {
            Label(error, style = Tokens.Type.MetaStrong, color = colors.statusFailed)
        }
    }
}

@Composable
fun Segmented(options: List<String>, selected: String, onSelect: (String) -> Unit, groupLabel: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Row(
        modifier
            .background(colors.bgPanel, ControlShape)
            .border(Tokens.Size.Hairline, colors.border, ControlShape)
            .padding(Tokens.Space.S0_5)
            .semantics { contentDescription = groupLabel },
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5),
    ) {
        for (option in options) {
            val on = option == selected

            Box(
                Modifier
                    .height(touchMin(Tokens.Size.ControlSm))
                    .pressable(
                        onClick = { onSelect(option) },
                        role = Role.RadioButton,
                        shape = SegmentShape,
                        selected = on,
                        ringInside = true,
                    )
                    .semantics { this.selected = on }
                    .padding(horizontal = Tokens.Space.S2),
                contentAlignment = Alignment.Center,
            ) {
                Label(option, style = Tokens.Type.Meta, color = if (on) colors.text else colors.textMuted, maxLines = 1)
            }
        }
    }
}

@Composable
fun TabRow(options: List<String>, selected: String, onSelect: (String) -> Unit, groupLabel: String, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Row(modifier.fillMaxWidth().semantics { contentDescription = groupLabel }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        for (option in options) {
            val on = option == selected

            Column(
                Modifier
                    .heightIn(min = rowHeight())
                    .pressable({ onSelect(option) }, role = Role.Tab, shape = ControlShape, ringInside = true)
                    .semantics { this.selected = on }
                    .padding(horizontal = Tokens.Space.S2),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Label(option, style = if (on) Tokens.Type.UiStrong else Tokens.Type.Ui, color = if (on) colors.text else colors.textMuted, maxLines = 1)
                Box(Modifier.width(Tokens.Space.S6).height(Tokens.Size.FocusRing).background(if (on) colors.accent else colors.bgContent))
            }
        }
    }
}

@Composable
fun MenuPopup(onDismiss: () -> Unit, modifier: Modifier = Modifier, width: Dp = Tokens.Size.Menu, content: @Composable () -> Unit) {
    val colors = MyTheme.colors

    if (LocalTouch.current) {
        BottomSheet(onDismiss) { content() }

        return
    }

    val gap = with(LocalDensity.current) { Tokens.Space.S1.roundToPx() }

    Popup(popupPositionProvider = remember(gap) { BelowAnchor(gap) }, onDismissRequest = onDismiss, properties = PopupProperties(focusable = true)) {
        Column(
            modifier
                .width(width)
                .shadow(Tokens.Space.S2, FloatShape, ambientColor = colors.shadowFloat, spotColor = colors.shadowFloat)
                .background(colors.bgRaised, FloatShape)
                .border(Tokens.Size.Hairline, colors.border, FloatShape)
                .padding(Tokens.Space.S1),
        ) {
            content()
        }
    }
}

internal class BelowAnchor(private val gap: Int, private val centred: Boolean = false) : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
        val fitsRight = anchorBounds.left + popupContentSize.width <= windowSize.width - gap
        val x = when {
            centred -> anchorBounds.center.x - popupContentSize.width / 2
            fitsRight -> anchorBounds.left
            else -> anchorBounds.right - popupContentSize.width
        }
        val fitsBelow = anchorBounds.bottom + gap + popupContentSize.height <= windowSize.height - gap
        val y = if (fitsBelow) anchorBounds.bottom + gap else anchorBounds.top - gap - popupContentSize.height

        return IntOffset(x.coerceIn(gap, maxOf(gap, windowSize.width - popupContentSize.width - gap)), y.coerceAtLeast(gap))
    }
}

@Composable
fun Tooltip(text: String, source: MutableInteractionSource) {
    val colors = MyTheme.colors
    val hovered by source.collectIsHoveredAsState()
    val gap = with(LocalDensity.current) { Tokens.Space.S1.roundToPx() }

    if (hovered) {
        Popup(popupPositionProvider = remember(gap) { BelowAnchor(gap, centred = true) }) {
            Box(
                Modifier
                    .background(colors.bgRaised, FloatShape)
                    .border(Tokens.Size.Hairline, colors.border, FloatShape)
                    .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
            ) {
                Label(text, style = Tokens.Type.Meta, maxLines = 1)
            }
        }
    }
}

private object ScreenPopup : PopupPositionProvider {
    override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset =
        IntOffset.Zero
}

@Composable
fun BottomSheet(onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    val colors = MyTheme.colors

    Popup(popupPositionProvider = ScreenPopup, onDismissRequest = onDismiss, properties = PopupProperties(focusable = true)) {
        Box(
            Modifier
                .fillMaxSize()
                .background(Color.Black.copy(alpha = 0.32f))
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClickLabel = "Close", onClick = onDismiss),
            contentAlignment = Alignment.BottomCenter,
        ) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .pointerInput(Unit) { detectTapGestures { } }
                    .background(colors.bgRaised, RoundedCornerShape(topStart = Tokens.Radius.Card, topEnd = Tokens.Radius.Card))
                    .navigationBarsPadding()
                    .verticalScroll(rememberScrollState())
                    .padding(vertical = Tokens.Space.S2),
                content = content,
            )
        }
    }
}

@Composable
fun Dialog(onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    val colors = MyTheme.colors

    if (LocalTouch.current) {
        BottomSheet(onDismiss) {
            Column(Modifier.padding(horizontal = Tokens.Space.S4, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4), content = content)
        }

        return
    }

    Popup(popupPositionProvider = ScreenPopup, onDismissRequest = onDismiss, properties = PopupProperties(focusable = true)) {
        Box(
            Modifier
                .fillMaxSize()
                .background(Color.Black.copy(alpha = 0.35f))
                .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClickLabel = "Close", onClick = onDismiss),
            contentAlignment = Alignment.Center,
        ) {
            Column(
                Modifier
                    .width(Tokens.Size.Dialog)
                    .pointerInput(Unit) { detectTapGestures { } }
                    .background(colors.bgRaised, FloatShape)
                    .border(Tokens.Size.Hairline, colors.border, FloatShape)
                    .padding(Tokens.Space.S4),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
                content = content,
            )
        }
    }
}

@Composable
fun MenuItem(label: String, onClick: () -> Unit, checked: Boolean = false, hint: String? = null, leading: MyIcon? = null) {
    val colors = MyTheme.colors

    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = rowHeight())
            .pressable(onClick, role = Role.RadioButton, shape = ControlShape, stateDescription = if (checked) "Selected" else null)
            .semantics { selected = checked }
            .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leading != null) {
            Icon(leading, colors.textMuted)
        }

        Label(label, maxLines = 1, overflow = TextOverflow.Visible)

        if (hint != null) {
            Label(hint, Modifier.weight(1f), Tokens.Type.Meta.copy(textAlign = TextAlign.End), colors.textMuted, maxLines = 1)
        } else {
            Box(Modifier.weight(1f))
        }

        if (checked) {
            Icon(MyIcon.Check, colors.text)
        }
    }
}

@Composable
fun MenuLabel(text: String) {
    Label(text, Modifier.padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1), Tokens.Type.Meta, MyTheme.colors.textMuted)
}

@Composable
fun SectionHead(text: String, modifier: Modifier = Modifier, aside: String? = null) {
    Row(
        modifier.fillMaxWidth().height(rowHeight()).padding(horizontal = Tokens.Space.S3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(text, Modifier.weight(1f).semantics { heading() }, Tokens.Type.UiStrong, maxLines = 1)

        if (aside != null) {
            Label(aside, style = Tokens.Type.Meta, color = MyTheme.colors.textMuted, maxLines = 1)
        }
    }
}

@Composable
fun ErrorBanner(message: String, onDismiss: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Row(
        modifier
            .fillMaxWidth()
            .background(colors.bgPanel)
            .padding(start = Tokens.Space.S3, end = Tokens.Space.S2)
            .semantics {
                contentDescription = "Error: $message"
                liveRegion = LiveRegionMode.Polite
            },
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm)
        Label(message, Modifier.weight(1f).padding(vertical = Tokens.Space.S1_5), Tokens.Type.Meta, colors.statusFailed)
        MyButton(onDismiss, icon = MyIcon.Close, description = "Dismiss the error", size = ButtonSize.Sm, muted = true)
    }
}

@Composable
fun CopyableCode(text: String, label: String, onCopy: (String) -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors

    Row(
        modifier
            .fillMaxWidth()
            .background(colors.bgPanel, ControlShape)
            .border(Tokens.Size.Hairline, colors.border, ControlShape)
            .padding(start = Tokens.Space.S3, top = Tokens.Space.S1_5, end = Tokens.Space.S1, bottom = Tokens.Space.S1_5),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(text, Modifier.weight(1f).horizontalScroll(rememberScrollState()), Tokens.Type.Code)
        MyButton({ onCopy(text) }, text = "Copy", icon = MyIcon.Copy, description = "Copy the ${label.lowercase()} command", size = ButtonSize.Sm, muted = true)
    }
}
