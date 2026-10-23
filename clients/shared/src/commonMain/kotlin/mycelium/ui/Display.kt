package mycelium.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.state.absoluteTime
import mycelium.state.ago
import mycelium.state.relativeTime

enum class TextSize(val label: String, val scale: Float) {
    Small("Small", 0.9f),
    Default("Default", 1f),
    Large("Large", 1.15f),
    ExtraLarge("Extra large", 1.3f),
}

enum class RowDensity(val label: String, val scale: Float) {
    Comfortable("Comfortable", 1f),
    Compact("Compact", 0.8f),
}

enum class ChatWidth(val label: String, val measure: Dp) {
    Narrow("Narrow", 560.dp),
    Standard("Standard", 720.dp),
    Wide("Wide", 920.dp),
    Full("Full", 4000.dp),
}

enum class CodeSize(val label: String, val scale: Float) {
    Smaller("Smaller", 0.9f),
    Regular("Regular", 1f),
    Bigger("Bigger", 1.2f),
}

enum class MotionSetting(val label: String) {
    System("System"),
    On("On"),
    Off("Off");

    fun reduced(system: Boolean): Boolean = when (this) {
        System -> system
        On -> true
        Off -> false
    }
}

enum class TimestampStyle(val label: String) {
    Relative("Relative"),
    Absolute("Absolute"),
}

data class Display(
    val textSize: TextSize = TextSize.Default,
    val density: RowDensity = RowDensity.Comfortable,
    val chatWidth: ChatWidth = ChatWidth.Standard,
    val codeSize: CodeSize = CodeSize.Regular,
    val reducedMotion: MotionSetting = MotionSetting.System,
    val timestamps: TimestampStyle = TimestampStyle.Relative,
)

@Composable
fun TextStyle.scaledForCode(): TextStyle {
    val scale = LocalAppearance.current.display.codeSize.scale

    return if (scale == 1f) this else copy(fontSize = fontSize * scale, lineHeight = lineHeight * scale)
}

@Composable
fun readingMeasure(): Dp = LocalAppearance.current.display.chatWidth.measure

@Composable
fun densed(base: Dp): Dp = maxOf(base * LocalAppearance.current.display.density.scale, Tokens.Size.DensityFloor)

@Composable
fun timestamper(): (String, Boolean) -> String {
    val style = LocalAppearance.current.display.timestamps
    val platform = LocalPlatform.current

    return { iso, inThePast ->
        when (style) {
            TimestampStyle.Relative -> if (inThePast) ago(iso) else relativeTime(iso)
            TimestampStyle.Absolute -> absoluteTime(iso) { millis -> platform?.localDateTime(millis) }
        }
    }
}

@Composable
fun timestamp(iso: String, ago: Boolean = false): String = timestamper()(iso, ago)
