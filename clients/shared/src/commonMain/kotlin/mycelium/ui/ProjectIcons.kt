package mycelium.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.painter.BitmapPainter
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import mycelium.api.ApiException
import mycelium.api.Project
import mycelium.state.PickedFile
import mycelium.state.Store
import mycelium.state.mediaTypeOf
import mycelium.state.projectLogoProblem
import org.jetbrains.compose.resources.decodeToImageBitmap

class ProjectIconSpec(val name: String, vararg val paths: String) {
    val label: String = name.replace('-', ' ')
}

val ProjectIconSet: List<ProjectIconSpec> = listOf(
    ProjectIconSpec("code", "m16 18 6-6-6-6M8 6l-6 6 6 6"),
    ProjectIconSpec("server", rect(2f, 3f, 20f, 8f, 2f), rect(2f, 13f, 20f, 8f, 2f), "M6 7h.01M6 17h.01"),
    ProjectIconSpec("database", "M3 5c0-1.66 4.03-3 9-3s9 1.34 9 3-4.03 3-9 3-9-1.34-9-3Z", "M3 5v14c0 1.66 4.03 3 9 3s9-1.34 9-3V5", "M3 12c0 1.66 4.03 3 9 3s9-1.34 9-3"),
    ProjectIconSpec("cloud", "M17.5 19H9a7 7 0 1 1 6.7-9h1.8a4.5 4.5 0 1 1 0 9Z"),
    ProjectIconSpec("globe", circle(12f, 12f, 9f), "M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"),
    ProjectIconSpec("phone", rect(7f, 2f, 10f, 20f, 2f), "M12 18h.01"),
    ProjectIconSpec("laptop", rect(3f, 4f, 18f, 12f, 2f), "M2 20h20"),
    ProjectIconSpec("camera", "M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3Z", circle(12f, 13f, 3f)),
    ProjectIconSpec("image", rect(3f, 3f, 18f, 18f, 2f), circle(9f, 9f, 2f), "m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"),
    ProjectIconSpec("music", "M9 18V5l12-2v13", circle(6f, 18f, 3f), circle(18f, 16f, 3f)),
    ProjectIconSpec("video", "m16 13 5.2 3.5a.5.5 0 0 0 .8-.4V7.9a.5.5 0 0 0-.8-.4L16 11", rect(2f, 6f, 14f, 12f, 2f)),
    ProjectIconSpec("book", "M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"),
    ProjectIconSpec("document", "M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z", "M14 3v6h6M16 13H8M16 17H8M10 9H8"),
    ProjectIconSpec("notebook", rect(4f, 2f, 16f, 20f, 2f), "M2 6h4M2 10h4M2 14h4M2 18h4M16 2v20"),
    ProjectIconSpec("calendar", rect(3f, 4f, 18f, 17f, 2f), "M3 10h18M8 2v4M16 2v4"),
    ProjectIconSpec("chat", "M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12Z"),
    ProjectIconSpec("mail", rect(2f, 4f, 20f, 16f, 2f), "m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"),
    ProjectIconSpec("bell", "M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9", "M10.3 21a1.94 1.94 0 0 0 3.4 0"),
    ProjectIconSpec("shield", "M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z"),
    ProjectIconSpec("lock", rect(3f, 11f, 18f, 11f, 2f), "M7 11V7a5 5 0 0 1 10 0v4"),
    ProjectIconSpec("key", circle(7.5f, 15.5f, 5.5f), "m21 2-9.6 9.6M15.5 7.5l3 3L22 7l-3-3"),
    ProjectIconSpec("user", circle(12f, 8f, 4f), "M4 21a8 8 0 0 1 16 0"),
    ProjectIconSpec("users", "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2", circle(9f, 7f, 4f), "M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75"),
    ProjectIconSpec("home", "m3 10 9-7 9 7v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z", "M9 22V12h6v10"),
    ProjectIconSpec("building", rect(4f, 2f, 16f, 20f, 2f), "M9 22v-4h6v4M8 6h.01M12 6h.01M16 6h.01M8 10h.01M12 10h.01M16 10h.01M8 14h.01M12 14h.01M16 14h.01"),
    ProjectIconSpec("map", "M9 3 3 5v16l6-2 6 2 6-2V3l-6 2Z", "M9 3v16M15 5v16"),
    ProjectIconSpec("compass", circle(12f, 12f, 9f), "m16 8-2 6-6 2 2-6Z"),
    ProjectIconSpec("leaf", "M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.48 19 2c1 2 2 4.18 2 8 0 5.5-4.78 10-10 10Z", "M2 21c0-3 1.85-5.36 5.08-6C9.5 14.52 12 13 13 12"),
    ProjectIconSpec("tree", "M12 22v-6", "M12 2 6 9h3l-4 7h14l-4-7h3Z"),
    ProjectIconSpec("flask", "M10 2v7.5L4.5 19a2 2 0 0 0 1.7 3h11.6a2 2 0 0 0 1.7-3L14 9.5V2M8.5 2h7M7 16h10"),
    ProjectIconSpec("beaker", "M4.5 3h15", "M6 3v16a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V3", "M6 14h12"),
    ProjectIconSpec("cube", "M21 8 12 3 3 8v8l9 5 9-5Z", "m3 8 9 5 9-5M12 13v8"),
    ProjectIconSpec("box", "M21 8v13H3V8", "M1 3h22v5H1Z", "M10 12h4"),
    ProjectIconSpec("package", "m7.5 4.27 9 5.15", "M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z", "m3.3 7 8.7 5 8.7-5M12 22V12"),
    ProjectIconSpec("layers", "m12 2 10 5-10 5L2 7Z", "m2 12 10 5 10-5", "m2 17 10 5 10-5"),
    ProjectIconSpec("chart", "M3 3v18h18", "M18 17V9M13 17V5M8 17v-3"),
    ProjectIconSpec("gauge", "m12 14 4-4", "M3.34 19a10 10 0 1 1 17.32 0"),
    ProjectIconSpec("bolt", "M13 2 3 14h9l-1 8 10-12h-9Z"),
    ProjectIconSpec("flame", "M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.07-2.14-.22-4.05 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.15.43-2.29 1-3a2.5 2.5 0 0 0 2.5 2.5Z"),
    ProjectIconSpec("heart", "M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z"),
    ProjectIconSpec("star", "m12 2 3.1 6.3 6.9 1-5 4.9 1.2 6.9-6.2-3.3-6.2 3.3 1.2-6.9-5-4.9 6.9-1Z"),
    ProjectIconSpec("game-controller", "M6 12h4M8 10v4M15 13h.01M18 11h.01", "M17.32 5H6.68a4 4 0 0 0-3.98 3.59C2.6 9.4 2 14.46 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.41-1.41A2 2 0 0 1 9.83 16h4.34a2 2 0 0 1 1.41.59L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.55-.6-6.6-.68-7.26A4 4 0 0 0 17.32 5Z"),
    ProjectIconSpec("wrench", "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94Z"),
    ProjectIconSpec("hammer", "m15 12-8.5 8.5a2.12 2.12 0 0 1-3-3L12 9", "M17.64 15 22 10.64", "m20.91 11.7-1.25-1.25c-.6-.6-.93-1.4-.93-2.25v-.86L16.01 4.6a5.56 5.56 0 0 0-3.94-1.64H9l.92.82A6.18 6.18 0 0 1 12 8.4v1.56l2 2h2.47l2.26 1.91"),
    ProjectIconSpec("terminal", rect(2f, 4f, 20f, 16f, 2f), "m7 10 3 2-3 2M13 15h4"),
    ProjectIconSpec("robot", rect(3f, 11f, 18f, 10f, 2f), circle(12f, 5f, 2f), "M12 7v4M8 16h.01M16 16h.01"),
    ProjectIconSpec("rocket", "M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09Z", "m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2Z", "M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0", "M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5"),
    ProjectIconSpec("money", circle(12f, 12f, 9f), "M15 9.5A3 3 0 0 0 12 8c-1.7 0-3 .9-3 2s1.3 1.6 3 2 3 .9 3 2-1.3 2-3 2a3 3 0 0 1-3-1.5M12 5.5V7M12 17v1.5"),
)

private val IconGridHeight = 200.dp

fun projectIconNamed(name: String?): ProjectIconSpec? = ProjectIconSet.firstOrNull { it.name == name }

@Composable
fun ProjectIcon(project: Project?, size: Dp = Tokens.Size.IconSm, modifier: Modifier = Modifier) {
    val sha256 = project?.avatarSha256
    val named = projectIconNamed(project?.icon)
    val loadBytes = LocalAttachmentBytes.current
    val avatar by produceState<Painter?>(null, sha256) {
        value = sha256?.let { loadBytes(it) }?.let { bytes -> runCatching { BitmapPainter(bytes.decodeToImageBitmap()) }.getOrNull() }
    }
    val drawn = avatar

    if (sha256 != null && drawn != null) {
        Image(drawn, contentDescription = null, modifier.size(size).clip(ControlShape), contentScale = ContentScale.Crop)
    } else if (sha256 == null && named != null) {
        StrokedPaths(named.paths.asList(), MyTheme.colors.textMuted, size, modifier)
    } else if (project != null) {
        val initial = project.name.firstOrNull { it.isLetterOrDigit() }?.uppercase() ?: "?"
        val fontSize = with(LocalDensity.current) { (size * 0.62f).toSp() }

        Box(modifier.size(size).border(Tokens.Size.Hairline, MyTheme.colors.textMuted, ControlShape), contentAlignment = Alignment.Center) {
            Label(initial, style = Tokens.Type.UiStrong.copy(fontSize = fontSize, lineHeight = fontSize), color = MyTheme.colors.textMuted)
        }
    } else {
        Box(modifier.size(size))
    }
}

@Composable
fun ProjectIconDialog(store: Store, projects: List<Project>, name: String, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val scope = rememberCoroutineScope()
    var choice by remember { mutableStateOf(projects.firstOrNull()?.icon) }
    var logo by remember { mutableStateOf<PickedFile?>(null) }
    var query by remember { mutableStateOf("") }
    var running by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    val shown = ProjectIconSet.filter { it.name.contains(query.trim().replace(' ', '-'), ignoreCase = true) }

    fun pickLogo(picked: List<PickedFile>) {
        val file = picked.firstOrNull() ?: return
        val named = PickedFile(file.name, mediaTypeOf(file.name, file.bytes, file.mediaType), file.bytes)
        val refused = projectLogoProblem(named)

        failure = refused

        if (refused == null) {
            logo = named
            choice = null
        }
    }

    fun save() {
        if (running) {
            return
        }

        failure = null
        running = true

        scope.launch {
            try {
                val chosenLogo = logo

                for (project in projects) {
                    if (chosenLogo != null) {
                        store.uploadProjectAvatar(project.id, chosenLogo.mediaType, chosenLogo.bytes)
                    } else if (project.icon != choice || project.avatarSha256 != null) {
                        store.setProjectIcon(project.id, choice)
                    }
                }

                onDismiss()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (problem: ApiException) {
                failure = problem.message
            } catch (_: Exception) {
                failure = "Could not reach the control plane. Check your connection and try again."
            } finally {
                running = false
            }
        }
    }

    Dialog(onDismiss) {
        Label("Project icon for $name", Modifier.semantics { heading() }, Tokens.Type.Title)

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            MyButton({ platform?.pickFiles(::pickLogo) }, text = "Upload logo", icon = MyIcon.Image, variant = ButtonVariant.Outline)
            Label(logo?.name ?: "PNG, JPEG or WebP", style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
        }

        TextInput(query, { query = it }, label = "Search icons", placeholder = "Search icons", showLabel = false, modifier = Modifier.fillMaxWidth())

        Column(Modifier.fillMaxWidth().heightIn(max = IconGridHeight).verticalScroll(rememberScrollState())) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                for (icon in shown) {
                    val selected = logo == null && icon.name == choice

                    IconChoice(icon, selected, onClick = { choice = icon.name; logo = null; failure = null })
                }
            }

            if (shown.isEmpty()) {
                Label("No icon matches.", color = colors.textMuted)
            }
        }

        failure?.let { ErrorBanner(it, onDismiss = { failure = null }) }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End)) {
            MyButton({ choice = null; logo = null; failure = null }, text = "No icon", muted = true, selected = choice == null && logo == null)
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Outline)
        }
    }
}

@Composable
private fun IconChoice(icon: ProjectIconSpec, selected: Boolean, onClick: () -> Unit) {
    val colors = MyTheme.colors
    val source = remember { MutableInteractionSource() }

    Box {
        Box(
            Modifier
                .size(touchMin(Tokens.Size.ControlLg))
                .border(Tokens.Size.Hairline, colors.borderControl, ControlShape)
                .pressable(onClick, role = Role.RadioButton, description = icon.label, shape = ControlShape, selected = selected, stateDescription = if (selected) "Selected" else null, source = source)
                .semantics { this.selected = selected },
            contentAlignment = Alignment.Center,
        ) {
            StrokedPaths(icon.paths.asList(), if (selected) colors.text else colors.textMuted, Tokens.Size.IconMd)
        }

        Tooltip(icon.label, source)
    }
}
