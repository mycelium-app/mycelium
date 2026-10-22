package mycelium.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.Dp
import mycelium.api.NodeSummary
import mycelium.state.isLaptop

private const val ViewBox = 24f
private const val StrokeWidth = 1.5f
private const val SpinMillis = 900

private fun circle(cx: Float, cy: Float, r: Float) =
    "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0"

private fun rect(x: Float, y: Float, w: Float, h: Float, r: Float) =
    "M${x + r} ${y}H${x + w - r}a$r $r 0 0 1 $r ${r}V${y + h - r}a$r $r 0 0 1 ${-r} ${r}H${x + r}a$r $r 0 0 1 ${-r} ${-r}V${y + r}a$r $r 0 0 1 $r ${-r}Z"

enum class MyIcon(vararg val paths: String) {
    ChevronDown("m6 9 6 6 6-6"),
    ChevronRight("m9 6 6 6-6 6"),
    Search(circle(11f, 11f, 7f), "m20 20-3.5-3.5"),
    Plus("M12 5v14M5 12h14"),
    Alert(circle(12f, 12f, 9f), "M12 8v4M12 16h.01"),
    Failed(circle(12f, 12f, 9f), "m15 9-6 6M9 9l6 6"),
    Review(circle(12f, 12f, 9f), circle(12f, 12f, 3f)),
    Working("M21 12a9 9 0 1 1-6.2-8.56"),
    Check("M20 6 9 17l-5-5"),
    Laptop(rect(3f, 4f, 18f, 12f, 2f), "M2 20h20"),
    Server(rect(2f, 3f, 20f, 8f, 2f), rect(2f, 13f, 20f, 8f, 2f), "M6 7h.01M6 17h.01"),
    Desktop(rect(2f, 3f, 20f, 13f, 2f), "M8 21h8M12 16v5"),
    Cloud("M17.5 19H9a7 7 0 1 1 6.7-9h1.8a4.5 4.5 0 1 1 0 9Z"),
    Terminal(rect(2f, 4f, 20f, 16f, 2f), "m7 10 3 2-3 2M13 15h4"),
    Chip(rect(5f, 5f, 14f, 14f, 2f), rect(9f, 9f, 6f, 6f, 1f), "M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"),
    Archive(rect(3f, 4f, 18f, 5f, 1f), "M5 9v10h14V9M10 13h4"),
    More("M5 12h.01M12 12h.01M19 12h.01"),
    Menu("M4 6h16M4 12h16M4 18h16"),
    Send("M22 2 11 13", "M22 2 15 22l-4-9-9-4Z"),
    Stop(rect(6f, 6f, 12f, 12f, 2f)),
    Pencil("M17 3a2.85 2.85 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"),
    Close("M18 6 6 18M6 6l12 12"),
    Copy(rect(9f, 9f, 13f, 13f, 2f), "M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"),
    Back("M19 12H5M12 19l-7-7 7-7"),
    Info(circle(12f, 12f, 9f), "M12 11v5M12 8h.01"),
    File("M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z", "M14 3v6h6"),
    Branch("M6 3v12", circle(18f, 6f, 3f), circle(6f, 18f, 3f), "M18 9a9 9 0 0 1-9 9"),
    Diff("M12 3v8M8 7h8M8 17h8"),
    Person(circle(12f, 8f, 4f), "M4 21a8 8 0 0 1 16 0"),
    Clock(circle(12f, 12f, 9f), "M12 7v5l3 2"),
    Agent(rect(3f, 11f, 18f, 10f, 2f), circle(12f, 5f, 2f), "M12 7v4M8 16h.01M16 16h.01"),
    Folder("M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"),
    Forge("M3 7h18l-2 10H5Z", "M9 7V4h6v3"),
    Mode("M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z"),
    Sliders("M4 6h10M18 6h2M4 12h2M10 12h10M4 18h12M20 18h0", circle(16f, 6f, 2f), circle(8f, 12f, 2f), circle(18f, 18f, 2f)),
    Logout("M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4M16 17l5-5-5-5M21 12H9"),
    Sun(circle(12f, 12f, 4f), "M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"),
    Moon("M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z"),
    ScrollToTop("M5 4h14M12 20V9M7 13l5-5 5 5"),
    ScrollToMessage(circle(9f, 7f, 4f), "M3 21v-1a5 5 0 0 1 5-5h2", "M18 21v-8M15 16l3-3 3 3"),
    ClaudeMark("M12 3v18M3 12h18M5.6 5.6l12.8 12.8M18.4 5.6 5.6 18.4"),
    CodexMark("M12 2.5 20.5 7.25v9.5L12 21.5l-8.5-4.75v-9.5Z", circle(12f, 12f, 3.5f)),
    Settings(circle(12f, 12f, 3f), "M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z"),
}

val NodeIcons: Map<String, MyIcon> by lazy {
    mapOf(
        "server" to MyIcon.Server,
        "laptop" to MyIcon.Laptop,
        "desktop" to MyIcon.Desktop,
        "cloud" to MyIcon.Cloud,
        "terminal" to MyIcon.Terminal,
        "chip" to MyIcon.Chip,
    )
}

fun nodeIcon(node: NodeSummary): MyIcon = NodeIcons[node.icon] ?: if (isLaptop(node.os)) MyIcon.Laptop else MyIcon.Server

fun agentIcon(agent: String): MyIcon = if (agent == "codex") MyIcon.CodexMark else MyIcon.ClaudeMark

@Composable
fun Icon(icon: MyIcon, tint: Color, size: Dp = Tokens.Size.IconMd, modifier: Modifier = Modifier) {
    val parsed = remember(icon) { icon.paths.map { PathParser().parsePathString(it).toPath() } }
    val spins = icon == MyIcon.Working && !LocalReducedMotion.current
    val turned = if (spins) {
        val angle by rememberInfiniteTransition().animateFloat(0f, 360f, infiniteRepeatable(tween(SpinMillis, easing = LinearEasing), RepeatMode.Restart))

        modifier.rotate(angle)
    } else {
        modifier
    }

    Canvas(turned.size(size)) {
        val scale = this.size.width / ViewBox

        withTransform({ scale(scale, scale, pivot = Offset.Zero) }) {
            for (path in parsed) {
                drawPath(path, tint, style = Stroke(width = StrokeWidth, cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
        }
    }
}
