package mycelium.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.state.InlineImageRef
import mycelium.state.ThreadImage
import mycelium.state.remoteThreadImage

private val PlaceholderWidth = 427.dp
private val RefusedWidth = 480.dp
private val LoadingHeight = 120.dp
private val CaptionHeight = 28.dp
private val DashLength = 6.dp
private val DashGap = 4.dp
private const val GalleryColumns = 4
private const val TouchGalleryColumns = 2
private const val NotFromSearch = "Not from an image search, so it loads only when you ask."

@Composable
fun ImagesBlock(images: List<InlineImageRef>) {
    if (LocalRemoteImages.current == null) {
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            for (image in images) {
                MarkdownText("![${image.alt}](${image.url})")
            }
        }

        return
    }

    if (images.size == 1) {
        SingleImage(images.single())
    } else {
        ImageGallery(images)
    }
}

@Composable
private fun SingleImage(ref: InlineImageRef) {
    val colors = MyTheme.colors
    val image = remember(ref) { remoteThreadImage(ref) }
    var pressed by remember(ref.url) { mutableStateOf(false) }
    val loaded by rememberLoadedImage(image, pressed)
    val focus = remember { FocusRequester() }

    LaunchedEffect(loaded) {
        if (pressed && (loaded is ImageState.Shown || loaded is ImageState.NotShown)) {
            focus.requestFocus()
        }
    }

    when (val state = loaded) {
        ImageState.Loading -> LoadingTile(Modifier.width(PlaceholderWidth).heightIn(min = LoadingHeight))
        is ImageState.Shown -> Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            OpensViewer(image, focus, Modifier.clip(ControlShape).border(Tokens.Size.Hairline, colors.border, ControlShape)) {
                Image(state.bitmap, contentDescription = null, Modifier.heightIn(max = Tokens.Size.Measure / 2.25f), contentScale = ContentScale.Fit)
            }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Label(ref.alt, Modifier.weight(1f, fill = false), Tokens.Type.Meta)
                HostLine(ref.host)
            }
        }
        is ImageState.NeedsClick -> LoadPlaceholder(ref, compact = false, onLoad = { pressed = true })
        is ImageState.NotShown -> Row(
            Modifier
                .widthIn(max = RefusedWidth)
                .fillMaxWidth()
                .border(Tokens.Size.Hairline, colors.border, BlockShape)
                .padding(Tokens.Space.S3),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        ) {
            Icon(state.icon, colors.textMuted)
            Column(Modifier.weight(1f)) {
                Label(ref.alt)
                Label("Not shown: ${state.reason}", style = Tokens.Type.Meta, color = colors.textMuted)
            }

            OpenLink(ref.url, Modifier.focusRequester(focus))
        }
    }
}

@Composable
private fun ImageGallery(images: List<InlineImageRef>) {
    val columns = if (LocalTouch.current) TouchGalleryColumns else GalleryColumns
    val gap = Tokens.Space.S2

    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val tileWidth = (maxWidth - gap * (columns - 1)) / columns

        Column(Modifier.semantics { contentDescription = "${images.size} images" }, verticalArrangement = Arrangement.spacedBy(gap)) {
            for (row in images.chunked(columns)) {
                Row(horizontalArrangement = Arrangement.spacedBy(gap)) {
                    for (ref in row) {
                        GalleryTile(ref, tileWidth)
                    }
                }
            }
        }
    }
}

@Composable
private fun GalleryTile(ref: InlineImageRef, tileWidth: Dp) {
    val colors = MyTheme.colors
    val image = remember(ref) { remoteThreadImage(ref) }
    var pressed by remember(ref.url) { mutableStateOf(false) }
    val loaded by rememberLoadedImage(image, pressed)
    val focus = remember { FocusRequester() }
    val tile = Modifier.width(tileWidth).clip(ControlShape).border(Tokens.Size.Hairline, colors.border, ControlShape).background(colors.bgPanel)

    LaunchedEffect(loaded) {
        if (pressed && (loaded is ImageState.Shown || loaded is ImageState.NotShown)) {
            focus.requestFocus()
        }
    }

    val content: @Composable () -> Unit = {
        Column {
            Box(Modifier.fillMaxWidth().aspectRatio(1f).background(colors.bgContent), contentAlignment = Alignment.Center) {
                when (val state = loaded) {
                    ImageState.Loading -> LoadingTile(Modifier)
                    is ImageState.Shown -> Image(state.bitmap, contentDescription = null, Modifier.fillMaxWidth(), contentScale = ContentScale.Fit)
                    is ImageState.NeedsClick -> LoadPlaceholder(ref, compact = true, onLoad = { pressed = true })
                    is ImageState.NotShown -> Column(Modifier.padding(Tokens.Space.S2), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                        Icon(state.icon, colors.textMuted)
                        Label("Not shown", style = Tokens.Type.UiStrong)
                        Label(state.reason, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 4, overflow = TextOverflow.Ellipsis)
                        OpenLink(ref.url, Modifier.focusRequester(focus))
                    }
                }
            }

            Box(Modifier.fillMaxWidth().heightIn(min = CaptionHeight).padding(horizontal = Tokens.Space.S2), contentAlignment = Alignment.CenterStart) {
                Label(ref.alt, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }
    }

    if (loaded is ImageState.Shown) {
        OpensViewer(image, focus, tile, content)
    } else {
        Box(tile) { content() }
    }
}

@Composable
private fun OpensViewer(image: ThreadImage, focus: FocusRequester, modifier: Modifier, content: @Composable () -> Unit) {
    var enlarged by remember { mutableStateOf(false) }
    var opened by remember { mutableStateOf(false) }

    LaunchedEffect(enlarged) {
        if (opened && !enlarged) {
            focus.requestFocus()
        }
    }

    Box(
        modifier
            .focusRequester(focus)
            .clickable(role = Role.Button) {
                opened = true
                enlarged = true
            }
            .semantics { contentDescription = "${image.name}, from ${image.host}. Open larger" },
    ) {
        content()
    }

    if (enlarged) {
        EnlargedImage(image, onClose = { enlarged = false })
    }
}

@Composable
private fun LoadingTile(modifier: Modifier) {
    val colors = MyTheme.colors

    Column(modifier.padding(Tokens.Space.S3), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.CenterVertically)) {
        Icon(MyIcon.Working, colors.statusWorking)
        Label("Loading image", style = Tokens.Type.Meta, color = colors.textMuted)
    }
}

@Composable
private fun HostLine(host: String) {
    val colors = MyTheme.colors

    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        Icon(MyIcon.Globe, colors.textMuted, Tokens.Size.IconSm)
        Label(host, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
    }
}

@Composable
private fun OpenLink(url: String, modifier: Modifier = Modifier) {
    val platform = LocalPlatform.current

    Box(modifier) {
        MyButton({ platform?.openUrl(url) }, text = "Open link", icon = MyIcon.External, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
    }
}

@Composable
private fun LoadPlaceholder(ref: InlineImageRef, compact: Boolean, onLoad: () -> Unit) {
    val colors = MyTheme.colors
    val dash = colors.borderControl
    val width = when {
        compact -> Modifier.fillMaxWidth()
        LocalTouch.current -> Modifier.fillMaxWidth()
        else -> Modifier.width(PlaceholderWidth)
    }

    Column(
        width
            .drawBehind {
                drawRoundRect(dash, cornerRadius = CornerRadius(Tokens.Radius.Block.toPx()), style = Stroke(Tokens.Size.Hairline.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(DashLength.toPx(), DashGap.toPx()))))
            }
            .padding(if (compact) Tokens.Space.S2 else Tokens.Space.S3)
            .semantics { contentDescription = "Image not loaded: ${ref.alt}" },
        horizontalAlignment = if (compact) Alignment.CenterHorizontally else Alignment.Start,
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.Top) {
            if (!compact) {
                Icon(MyIcon.Image, colors.textMuted, Tokens.Size.IconSm, Modifier.padding(top = Tokens.Space.S0_5))
            }

            Label(ref.alt, Modifier.weight(1f, fill = false), if (compact) Tokens.Type.Meta else Tokens.Type.Ui, maxLines = if (compact) 2 else Int.MAX_VALUE)
        }

        HostLine(ref.host)

        if (!compact) {
            Label(NotFromSearch, style = Tokens.Type.Meta, color = colors.textMuted)
        }

        MyButton(onLoad, text = "Load image from ${ref.host}", icon = MyIcon.Image, description = "Load image from ${ref.host}: ${ref.alt}", variant = ButtonVariant.Outline, size = ButtonSize.Sm, wraps = compact)
    }
}

@Composable
fun ImageCaption(alt: String, host: String, pageUrl: String?, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val touch = LocalTouch.current

    val text: @Composable (Modifier) -> Unit = { weight ->
        Column(weight) {
            Label(alt)
            HostLine(host)
        }
    }
    val open: @Composable () -> Unit = {
        if (pageUrl != null) {
            MyButton({ platform?.openUrl(pageUrl) }, text = "Open page", icon = MyIcon.External, variant = ButtonVariant.Outline, size = ButtonSize.Sm)
        }
    }

    val bar = modifier
        .widthIn(max = Tokens.Size.Measure)
        .fillMaxWidth()
        .background(colors.bgPanel, BlockShape)
        .border(Tokens.Size.Hairline, colors.border, BlockShape)
        .padding(Tokens.Space.S3)

    if (touch) {
        Column(bar, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            text(Modifier)
            open()
        }
    } else {
        Row(bar, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3), verticalAlignment = Alignment.CenterVertically) {
            text(Modifier.weight(1f))
            open()
        }
    }
}

private val ImageState.NotShown.icon get() = if (privateAddress) MyIcon.Shield else MyIcon.ImageOff
