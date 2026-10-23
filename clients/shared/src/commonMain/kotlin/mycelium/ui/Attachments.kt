package mycelium.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import mycelium.api.Attachment
import mycelium.state.DraftAttachment
import mycelium.state.PickedFile
import mycelium.state.ThreadImage
import mycelium.state.attachmentProblem
import mycelium.state.mediaTypeOf
import mycelium.state.decodeBase64
import mycelium.state.formatSize
import mycelium.state.isImage
import org.jetbrains.compose.resources.decodeToImageBitmap

class AttachmentControls(
    val drafts: List<DraftAttachment>,
    val problem: String?,
    val onAdd: (List<PickedFile>) -> Unit,
    val onRemove: (DraftAttachment) -> Unit,
)

class AttachmentDrafts(initial: List<DraftAttachment> = emptyList()) {
    var drafts by mutableStateOf(initial)
        private set
    var problem by mutableStateOf<String?>(null)
        private set
    private var nextId = (initial.maxOfOrNull { it.id } ?: -1) + 1

    fun add(picked: List<PickedFile>) {
        var held = drafts
        problem = null

        for (file in picked) {
            val named = PickedFile(file.name, mediaTypeOf(file.name, file.bytes, file.mediaType), file.bytes)
            val refused = attachmentProblem(named, held.size)

            if (refused != null) {
                problem = refused
            } else {
                held = held + DraftAttachment(nextId++, named)
            }
        }

        drafts = held
    }

    fun remove(draft: DraftAttachment) {
        drafts = drafts - draft
        problem = null
    }

    fun clear() {
        drafts = emptyList()
        problem = null
    }

    fun controls() = AttachmentControls(drafts, problem, ::add, ::remove)
}

@Composable
fun rememberAttachmentDrafts(key: Any? = null, initial: List<DraftAttachment> = emptyList()): AttachmentDrafts = remember(key) { AttachmentDrafts(initial) }

val LocalAttachmentBytes = staticCompositionLocalOf<suspend (String) -> ByteArray?> { { null } }

val LocalImageJump = staticCompositionLocalOf<(String) -> Unit> { {} }

val LocalImageGallery = staticCompositionLocalOf<List<ThreadImage>> { emptyList() }

private fun bitmapOf(bytes: ByteArray): ImageBitmap? = runCatching { bytes.decodeToImageBitmap() }.getOrNull()

@Composable
fun rememberLoadedImage(image: ThreadImage): State<Pair<Boolean, ImageBitmap?>> {
    val loadBytes = LocalAttachmentBytes.current

    return produceState<Pair<Boolean, ImageBitmap?>>(false to null, image.sha256) {
        val bytes = image.data?.let(::decodeBase64) ?: loadBytes(image.sha256)

        value = true to bytes?.let(::bitmapOf)
    }
}

@Composable
fun AttachmentChips(controls: AttachmentControls) {
    val colors = MyTheme.colors

    if (controls.drafts.isEmpty() && controls.problem == null) {
        return
    }

    Column(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            for (draft in controls.drafts) {
                val file = draft.file
                val size = formatSize(file.bytes.size.toLong())
                val thumbnail = remember(draft.id) { if (isImage(file.mediaType)) bitmapOf(file.bytes) else null }

                Row(
                    Modifier
                        .border(Tokens.Size.Hairline, colors.border, ControlShape)
                        .padding(start = Tokens.Space.S1)
                        .semantics { contentDescription = "Attached: ${file.name}, $size" },
                    horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (thumbnail != null) {
                        Image(thumbnail, contentDescription = null, Modifier.size(Tokens.Size.ControlMd).clip(ControlShape), contentScale = ContentScale.Crop)
                    } else {
                        Icon(if (isImage(file.mediaType)) MyIcon.Image else MyIcon.File, colors.textMuted, Tokens.Size.IconSm)
                    }

                    Label(file.name, Modifier.padding(start = Tokens.Space.S0_5), Tokens.Type.Meta, maxLines = 1)

                    if (thumbnail == null) {
                        Label(size, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                    }

                    MyButton({ controls.onRemove(draft) }, icon = MyIcon.Close, description = "Remove attachment: ${file.name}", size = ButtonSize.Sm, muted = true)
                }
            }
        }

        controls.problem?.let { problem ->
            Row(Modifier.semantics { liveRegion = LiveRegionMode.Assertive }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                Icon(MyIcon.Failed, colors.statusFailed, Tokens.Size.IconSm)
                Label(problem, style = Tokens.Type.Meta, color = colors.statusFailed)
            }
        }
    }
}

@Composable
fun MessageAttachments(files: List<Attachment>, images: List<ThreadImage>) {
    val colors = MyTheme.colors

    Column(Modifier.fillMaxWidth().padding(top = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        if (images.isNotEmpty()) {
            Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                for (image in images) {
                    ThreadImageView(image, image.name)
                }
            }
        }

        for (file in files) {
            Row(
                Modifier
                    .border(Tokens.Size.Hairline, colors.border, ControlShape)
                    .padding(horizontal = Tokens.Space.S2, vertical = Tokens.Space.S1)
                    .semantics { contentDescription = "Attachment: ${file.name}, ${formatSize(file.size)}" },
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(MyIcon.File, colors.textMuted, Tokens.Size.IconSm)
                Label(file.name, style = Tokens.Type.Meta, maxLines = 1)
                Label(formatSize(file.size), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }
        }
    }
}

@Composable
fun ThreadImageView(image: ThreadImage, alt: String) {
    val colors = MyTheme.colors
    val jump = LocalImageJump.current
    val sameAs = image.sameAs

    if (sameAs != null) {
        MyButton({ jump(sameAs) }, text = "Same image as above", icon = MyIcon.Image, size = ButtonSize.Sm, muted = true)

        return
    }

    val loaded by rememberLoadedImage(image)
    val bitmap = loaded.second
    var enlarged by remember { mutableStateOf(false) }

    when {
        bitmap != null -> Image(
            bitmap,
            contentDescription = alt,
            Modifier
                .heightIn(max = Tokens.Size.Measure / 3)
                .clip(ControlShape)
                .border(Tokens.Size.Hairline, colors.border, ControlShape)
                .clickable(onClickLabel = "Open larger", role = Role.Button) { enlarged = true },
            contentScale = ContentScale.Fit,
        )
        loaded.first -> Label("$alt could not be shown", style = Tokens.Type.Meta, color = colors.textMuted)
        else -> Label("Loading $alt", style = Tokens.Type.Meta, color = colors.textMuted)
    }

    if (enlarged && bitmap != null) {
        val gallery = LocalImageGallery.current.takeIf { list -> list.any { it.sha256 == image.sha256 } } ?: listOf(image)

        Dialog(onDismissRequest = { enlarged = false }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
            ImageViewer(gallery, gallery.indexOfFirst { it.sha256 == image.sha256 }, onClose = { enlarged = false })
        }
    }
}
