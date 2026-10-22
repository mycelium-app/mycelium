package mycelium.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.Dp

private const val ViewBox = 24f
private const val StrokeWidth = 1.5f

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
    Archive(rect(3f, 4f, 18f, 5f, 1f), "M5 9v10h14V9M10 13h4"),
    More("M5 12h.01M12 12h.01M19 12h.01"),
    Send("M12 19V5M5 12l7-7 7 7"),
    Stop(rect(6f, 6f, 12f, 12f, 2f)),
    Pencil("M17 3a2.85 2.85 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"),
    Close("M18 6 6 18M6 6l12 12"),
    Copy(rect(9f, 9f, 13f, 13f, 2f), "M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"),
    Back("M19 12H5M12 19l-7-7 7-7"),
    Info(circle(12f, 12f, 9f), "M12 11v5M12 8h.01"),
    File("M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z", "M14 3v6h6"),
    Branch("M6 3v12", circle(18f, 6f, 3f), circle(6f, 18f, 3f), "M18 9a9 9 0 0 1-9 9"),
    Clock(circle(12f, 12f, 9f), "M12 7v5l3 2"),
    Agent(rect(3f, 11f, 18f, 10f, 2f), circle(12f, 5f, 2f), "M12 7v4M8 16h.01M16 16h.01"),
    Folder("M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"),
    Mode("M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z"),
    Sliders("M4 6h10M18 6h2M4 12h2M10 12h10M4 18h12M20 18h0", circle(16f, 6f, 2f), circle(8f, 12f, 2f), circle(18f, 18f, 2f)),
    Logout("M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4M16 17l5-5-5-5M21 12H9"),
}

@Composable
fun Icon(icon: MyIcon, tint: Color, size: Dp = Tokens.Size.IconMd, modifier: Modifier = Modifier) {
    val parsed = remember(icon) { icon.paths.map { PathParser().parsePathString(it).toPath() } }

    Canvas(modifier.size(size)) {
        val scale = this.size.width / ViewBox

        withTransform({ scale(scale, scale, pivot = Offset.Zero) }) {
            for (path in parsed) {
                drawPath(path, tint, style = Stroke(width = StrokeWidth, cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
        }
    }
}
