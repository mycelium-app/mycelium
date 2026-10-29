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
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.drawscope.Fill
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.Dp
import mycelium.api.NodeSummary
import mycelium.state.isLaptop

private const val ViewBox = 24f
private const val StrokeWidth = 1.5f
private const val SpinMillis = 900

private val MyIcon.filled: Boolean get() = this == MyIcon.ClaudeMark

internal fun circle(cx: Float, cy: Float, r: Float) =
    "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0"

internal fun rect(x: Float, y: Float, w: Float, h: Float, r: Float) =
    "M${x + r} ${y}H${x + w - r}a$r $r 0 0 1 $r ${r}V${y + h - r}a$r $r 0 0 1 ${-r} ${r}H${x + r}a$r $r 0 0 1 ${-r} ${-r}V${y + r}a$r $r 0 0 1 $r ${-r}Z"

enum class MyIcon(vararg val paths: String) {
    ChevronDown("m6 9 6 6 6-6"),
    ArrowUp("M12 19V5M5 12l7-7 7 7"),
    ArrowDown("M12 5v14M19 12l-7 7-7-7"),
    ChevronLeft("m15 6-6 6 6 6"),
    ChevronRight("m9 6 6 6-6 6"),
    Search(circle(11f, 11f, 7f), "m20 20-3.5-3.5"),
    Plus("M12 5v14M5 12h14"),
    GraduationCap("M21.42 10.922a1 1 0 0 0-.019-1.838L12.83 5.18a2 2 0 0 0-1.66 0L2.6 9.08a1 1 0 0 0 0 1.832l8.57 3.908a2 2 0 0 0 1.66 0z", "M22 10v6", "M6 12.5V16a6 3 0 0 0 12 0v-3.5"),
    Leaf("M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.48 19 2c1 2 2 4.18 2 8 0 5.5-4.78 10-10 10Z", "M2 21c0-3 1.85-5.36 5.08-6C9.5 14.52 12 13 13 12"),
    Zap("M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"),
    Cup("M10 2v2M14 2v2M6 2v2", "M16 8h1a4 4 0 1 1 0 8h-1", "M4 8h12v9a4 4 0 0 1-4 4H8a4 4 0 0 1-4-4Z"),
    Minus("M5 12h14"),
    Alert(circle(12f, 12f, 9f), "M12 8v4M12 16h.01"),
    Question(circle(12f, 12f, 9f), "M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3M12 17h.01"),
    Lightbulb("M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5", "M9 18h6M10 22h4"),
    TriangleAlert("m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3", "M12 9v4M12 17h.01"),
    Failed(circle(12f, 12f, 9f), "m15 9-6 6M9 9l6 6"),
    Review(circle(12f, 12f, 9f), circle(12f, 12f, 3f)),
    Working("M21 12a9 9 0 1 1-6.2-8.56"),
    Check("M20 6 9 17l-5-5"),
    Star("M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"),
    Laptop(rect(3f, 4f, 18f, 12f, 2f), "M2 20h20"),
    List("M9 6h12M9 12h12M9 18h12M4 6h.01M4 12h.01M4 18h.01"),
    Calendar(rect(3f, 4f, 18f, 17f, 2f), "M3 10h18M8 2v4M16 2v4"),
    Server(rect(2f, 3f, 20f, 8f, 2f), rect(2f, 13f, 20f, 8f, 2f), "M6 7h.01M6 17h.01"),
    Desktop(rect(2f, 3f, 20f, 13f, 2f), "M8 21h8M12 16v5"),
    Cloud("M17.5 19H9a7 7 0 1 1 6.7-9h1.8a4.5 4.5 0 1 1 0 9Z"),
    Terminal(rect(2f, 4f, 20f, 16f, 2f), "m7 10 3 2-3 2M13 15h4"),
    Chip(rect(5f, 5f, 14f, 14f, 2f), rect(9f, 9f, 6f, 6f, 1f), "M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"),
    Archive(rect(3f, 4f, 18f, 5f, 1f), "M5 9v10h14V9M10 13h4"),
    Pin("M12 17v5", "M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"),
    More("M5 12h.01M12 12h.01M19 12h.01"),
    MoreVertical("M12 5h.01M12 12h.01M12 19h.01"),
    Grip("M9 5h.01M9 12h.01M9 19h.01M15 5h.01M15 12h.01M15 19h.01"),
    Menu("M4 6h16M4 12h16M4 18h16"),
    PanelLeft("M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z", "M9 3v18"),
    Send("M22 2 11 13", "M22 2 15 22l-4-9-9-4Z"),
    Microphone(rect(9f, 2f, 6f, 13f, 3f), "M19 10v2a7 7 0 0 1-14 0v-2", "M12 19v3"),
    Speaker("M11 5 6 9H2v6h4l5 4Z", "M15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14"),
    Paperclip("m21.44 11.05-9.19 9.19a6.003 6.003 0 0 1-8.49-8.49l8.57-8.57A4.006 4.006 0 0 1 17.98 9.1l-8.59 8.57a2.002 2.002 0 0 1-2.83-2.83l8.49-8.48"),
    Image(rect(3f, 3f, 18f, 18f, 2f), circle(9f, 9f, 2f), "m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"),
    Stop(rect(6f, 6f, 12f, 12f, 2f)),
    Play("m6 3 14 9-14 9Z"),
    Pencil("M17 3a2.85 2.85 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"),
    Close("M18 6 6 18M6 6l12 12"),
    Copy(rect(9f, 9f, 13f, 13f, 2f), "M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"),
    Back("M19 12H5M12 19l-7-7 7-7"),
    Info(circle(12f, 12f, 9f), "M12 11v5M12 8h.01"),
    Layers("m12 3 9 5-9 5-9-5Z", "m3 13 9 5 9-5"),
    Workflow("M5 3H9a2 2 0 0 1 2 2V9a2 2 0 0 1 -2 2H5a2 2 0 0 1 -2 -2V5a2 2 0 0 1 2 -2Z", "M7 11v4a2 2 0 0 0 2 2h4", "M15 13H19a2 2 0 0 1 2 2V19a2 2 0 0 1 -2 2H15a2 2 0 0 1 -2 -2V15a2 2 0 0 1 2 -2Z"),
    Hand("M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2", "M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2", "M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8", "M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15"),
    File("M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z", "M14 3v6h6"),
    PenTool("M15.707 21.293a1 1 0 0 1-1.414 0l-1.586-1.586a1 1 0 0 1 0-1.414l5.586-5.586a1 1 0 0 1 1.414 0l1.586 1.586a1 1 0 0 1 0 1.414z", "m18 13-1.375-6.874a1 1 0 0 0-.746-.776L3.235 2.028a1 1 0 0 0-1.207 1.207L5.35 15.879a1 1 0 0 0 .776.746L13 18", "m2.3 2.3 7.286 7.286", circle(11f, 11f, 2f)),
    TypeText("M12 4v16", "M4 7V5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v2", "M9 20h6"),
    FileText("M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z", "M14 2v5a1 1 0 0 0 1 1h5", "M10 9H8M16 13H8M16 17H8"),
    Presentation("M2 3h20", "M21 3v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V3", "m7 21 5-5 5 5"),
    Chart("M3 3v16a2 2 0 0 0 2 2h16", "m19 9-5 5-4-4-3 3"),
    Film(rect(2f, 2f, 20f, 20f, 2.18f), "M7 2v20M17 2v20M2 12h20M2 7h5M2 17h5M17 17h5M17 7h5"),
    Palette("M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z", "M13.5 6.5h.01M17.5 10.5h.01M6.5 12.5h.01M8.5 7.5h.01"),
    Tag("M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42Z", circle(7.5f, 7.5f, 1f)),
    Branch("M6 3v12", circle(18f, 6f, 3f), circle(6f, 18f, 3f), "M18 9a9 9 0 0 1-9 9"),
    Diff("M12 3v8M8 7h8M8 17h8"),
    Person(circle(12f, 8f, 4f), "M4 21a8 8 0 0 1 16 0"),
    Clock(circle(12f, 12f, 9f), "M12 7v5l3 2"),
    Agent(rect(3f, 11f, 18f, 10f, 2f), circle(12f, 5f, 2f), "M12 7v4M8 16h.01M16 16h.01"),
    Activity("M3 12h4l3-8 4 16 3-8h4"),
    Folder("M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"),
    Forge("M3 7h18l-2 10H5Z", "M9 7V4h6v3"),
    Issue(circle(12f, 12f, 9f), circle(12f, 12f, 1.5f)),
    Gauge("M12 14l4-4", "M3.3 19a10 10 0 1 1 17.4 0"),
    Mode("M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z"),
    Sliders("M4 6h10M18 6h2M4 12h2M10 12h10M4 18h12M20 18h0", circle(16f, 6f, 2f), circle(8f, 12f, 2f), circle(18f, 18f, 2f)),
    Logout("M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4M16 17l5-5-5-5M21 12H9"),
    Sun(circle(12f, 12f, 4f), "M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"),
    Moon("M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z"),
    ScrollToTop("M5 4h14M12 20V9M7 13l5-5 5 5"),
    ScrollToBottom("M5 20h14M12 4v11M7 11l5 5 5-5"),
    ScrollToMessage(circle(9f, 7f, 4f), "M3 21v-1a5 5 0 0 1 5-5h2", "M18 21v-8M15 16l3-3 3 3"),
    ClaudeMark("m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"),
    CodexMark("M12 2.5 20.5 7.25v9.5L12 21.5l-8.5-4.75v-9.5Z", circle(12f, 12f, 3.5f)),
    Key(circle(7.5f, 15.5f, 5.5f), "m21 2-9.6 9.6M15.5 7.5l3 3L22 7l-3-3"),
    Globe(circle(12f, 12f, 9f), "M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"),
    Phone(rect(7f, 2f, 10f, 20f, 2f), "M12 18h.01"),
    Settings(circle(12f, 12f, 3f), "M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z"),
    Move("M5 12h14M13 6l6 6-6 6"),
    Target(circle(12f, 12f, 9f), circle(12f, 12f, 5f), circle(12f, 12f, 1f)),
    Expand("M8 3H5a2 2 0 0 0-2 2v3", "M21 8V5a2 2 0 0 0-2-2h-3", "M3 16v3a2 2 0 0 0 2 2h3", "M16 21h3a2 2 0 0 0 2-2v-3"),
    Pointer("M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z"),
    Refresh("M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5", "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5"),
    Code("m16 18 6-6-6-6M8 6l-6 6 6 6"),
    Flask("M10 2v7.5L4.5 19a2 2 0 0 0 1.7 3h11.6a2 2 0 0 0 1.7-3L14 9.5V2M8.5 2h7M7 16h10"),
    Chat("M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12Z"),
    Memory("M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 0 1 0 12 18Z", "M12 5a3 3 0 1 1 5.997.125 4 4 0 0 1 2.526 5.77 4 4 0 0 1-.556 6.588A4 4 0 1 1 12 18Z", "M12 5v13"),
    Task(rect(3f, 3f, 18f, 18f, 2f), "m9 12 2 2 4-4"),
    AppWindow(rect(2f, 4f, 20f, 16f, 2f), "M10 4v4M2 8h20M6 4v4"),
    ChefHat("M17 21a1 1 0 0 0 1-1v-5.35c0-.457.316-.844.727-1.041a4 4 0 0 0-2.134-7.589 5 5 0 0 0-9.186 0 4 4 0 0 0-2.134 7.588c.411.198.727.585.727 1.041V20a1 1 0 0 0 1 1Z", "M6 17h12"),
    Receipt("M4 2v20l2-1 2 1 2-1 2 1 2-1 2 1 2-1 2 1V2l-2 1-2-1-2 1-2-1-2 1-2-1-2 1Z", "M16 8h-6a2 2 0 1 0 0 4h4a2 2 0 1 1 0 4H8", "M12 17.5v-11"),
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

fun nodeIcon(node: NodeSummary): MyIcon = NodeIcons[node.icon] ?: when {
    node.kubernetes != null -> MyIcon.Cloud
    isLaptop(node.os) -> MyIcon.Laptop
    else -> MyIcon.Server
}

fun agentIcon(agent: String): MyIcon = when (agent) {
    "codex" -> MyIcon.CodexMark
    "chat" -> MyIcon.Chat
    else -> MyIcon.ClaudeMark
}

@Composable
fun Icon(icon: MyIcon, tint: Color, size: Dp = Tokens.Size.IconMd, modifier: Modifier = Modifier) {
    val spins = icon == MyIcon.Working && !LocalReducedMotion.current
    val turned = if (spins) {
        val angle by rememberInfiniteTransition().animateFloat(0f, 360f, infiniteRepeatable(tween(SpinMillis, easing = LinearEasing), RepeatMode.Restart))

        modifier.rotate(angle)
    } else {
        modifier
    }

    StrokedPaths(icon.paths.asList(), tint, size, turned, filled = icon.filled)
}

@Composable
internal fun StrokedPaths(paths: List<String>, tint: Color, size: Dp, modifier: Modifier = Modifier, filled: Boolean = false) {
    val parsed = remember(paths) { paths.map { PathParser().parsePathString(it).toPath().apply { fillType = PathFillType.NonZero } } }

    Canvas(modifier.size(size)) {
        val scale = this.size.width / ViewBox

        withTransform({ scale(scale, scale, pivot = Offset.Zero) }) {
            for (path in parsed) {
                drawPath(path, tint, style = if (filled) Fill else Stroke(width = StrokeWidth, cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
        }
    }
}
