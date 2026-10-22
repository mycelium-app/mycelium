package mycelium.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import mycelium.resources.Res
import mycelium.resources.ibmplexmono_medium
import mycelium.resources.ibmplexmono_regular
import mycelium.resources.ibmplexsans_medium
import mycelium.resources.ibmplexsans_regular
import mycelium.resources.ibmplexsans_semibold
import org.jetbrains.compose.resources.Font

data class Palette(
    val bgContent: Color,
    val bgPanel: Color,
    val bgChrome: Color,
    val bgRaised: Color,
    val bgHover: Color,
    val bgSelected: Color,
    val border: Color,
    val borderControl: Color,
    val text: Color,
    val textMuted: Color,
    val textFaint: Color,
    val accent: Color,
    val onAccent: Color,
    val statusWorking: Color,
    val statusAttention: Color,
    val statusFailed: Color,
    val diffAddBg: Color,
    val diffDelBg: Color,
    val diffAdd: Color,
    val diffDel: Color,
    val shadowFloat: Color,
) {
    val focus: Color get() = accent
}

enum class Accent(val label: String, val dark: Color, val light: Color) {
    System("System", Color(0xFFE6E6E6), Color(0xFF171717)),
    Neutral("Neutral", Color(0xFFE6E6E6), Color(0xFF171717)),
    Blue("Blue", Color(0xFF8AB8F0), Color(0xFF1F58A8)),
    Violet("Violet", Color(0xFFB9A3F2), Color(0xFF5B3FB0)),
    Amber("Amber", Color(0xFFE6BC6C), Color(0xFF7A5000)),
    Rose("Rose", Color(0xFFEE93AE), Color(0xFFA32F57)),
    Green("Green", Color(0xFF7CCBA0), Color(0xFF1B6A44)),
}

fun palette(dark: Boolean, accent: Accent, systemAccent: Color? = null): Palette {
    val chosen = if (accent == Accent.System && systemAccent != null) systemAccent else if (dark) accent.dark else accent.light
    val base = if (dark) Tokens.Dark else Tokens.Light

    return base.copy(accent = chosen, onAccent = if (chosen.luminance() > 0.4f) Color(0xFF111111) else Color(0xFFFFFFFF))
}

object Tokens {
    val Dark = Palette(
        bgContent = Color(0xFF161616),
        bgPanel = Color(0xFF1A1A1A),
        bgChrome = Color(0xFF1F1F1F),
        bgRaised = Color(0xFF262626),
        bgHover = Color(0xFF262626),
        bgSelected = Color(0xFF303030),
        border = Color(0xFF2C2C2C),
        borderControl = Color(0xFF6B6B6B),
        text = Color(0xFFE6E6E6),
        textMuted = Color(0xFFA8A8A8),
        textFaint = Color(0xFF9C9C9C),
        accent = Accent.Neutral.dark,
        onAccent = Color(0xFF111111),
        statusWorking = Color(0xFF7FB2EC),
        statusAttention = Color(0xFFE2B45F),
        statusFailed = Color(0xFFEC8576),
        diffAddBg = Color(0xFF17291F),
        diffDelBg = Color(0xFF2F1C19),
        diffAdd = Color(0xFF7CCBA0),
        diffDel = Color(0xFFEC8576),
        shadowFloat = Color(0x80000000),
    )

    val Light = Palette(
        bgContent = Color(0xFFFFFFFF),
        bgPanel = Color(0xFFF4F4F4),
        bgChrome = Color(0xFFEDEDED),
        bgRaised = Color(0xFFFFFFFF),
        bgHover = Color(0xFFE7E7E7),
        bgSelected = Color(0xFFDDDDDD),
        border = Color(0xFFDFDFDF),
        borderControl = Color(0xFF7E7E7E),
        text = Color(0xFF171717),
        textMuted = Color(0xFF5A5A5A),
        textFaint = Color(0xFF5E5E5E),
        accent = Accent.Neutral.light,
        onAccent = Color(0xFFFFFFFF),
        statusWorking = Color(0xFF1F58A8),
        statusAttention = Color(0xFF875800),
        statusFailed = Color(0xFFB0372A),
        diffAddBg = Color(0xFFE6F4EA),
        diffDelBg = Color(0xFFFBE7E3),
        diffAdd = Color(0xFF1B6A44),
        diffDel = Color(0xFFB0372A),
        shadowFloat = Color(0x29171717),
    )

    object Space {
        val S0_5: Dp = 2.dp
        val S1: Dp = 4.dp
        val S1_5: Dp = 6.dp
        val S2: Dp = 8.dp
        val S3: Dp = 12.dp
        val S4: Dp = 16.dp
        val S6: Dp = 24.dp
        val S8: Dp = 32.dp
    }

    object Size {
        val ControlSm: Dp = 22.dp
        val ControlMd: Dp = 28.dp
        val ControlLg: Dp = 32.dp
        val Row: Dp = 32.dp
        val Bar: Dp = 40.dp
        val DiffFile: Dp = 30.dp
        val IconSm: Dp = 14.dp
        val IconMd: Dp = 16.dp
        val Dot: Dp = 6.dp
        val Sidebar: Dp = 280.dp
        val SidebarMin: Dp = 200.dp
        val SidebarMax: Dp = 480.dp
        val Inspector: Dp = 296.dp
        val Touch: Dp = 44.dp
        val BarTouch: Dp = 52.dp
        val Rail: Dp = 44.dp
        val Menu: Dp = 280.dp
        val MenuList: Dp = 360.dp
        val Measure: Dp = 720.dp
        val Hairline: Dp = 1.dp
        val FocusRing: Dp = 2.dp
        val FocusOffset: Dp = 1.dp
        val DiffNumber: Dp = 36.dp
        val DiffSign: Dp = 16.dp
        val ComposerInput: Dp = 44.dp
        val ProgressBar: Dp = 4.dp
        val FormColumn: Dp = 320.dp
        val ApproveColumn: Dp = 420.dp
        val Popover: Dp = 440.dp
        val LoginPopover: Dp = 360.dp
        val NumberField: Dp = 72.dp
        val ActionColumn: Dp = 200.dp
        val Page: Dp = 640.dp
        val Compact: Dp = 600.dp
    }

    object Radius {
        val Control: Dp = 4.dp
        val Block: Dp = 6.dp
        val Float: Dp = 8.dp
        val Card: Dp = 16.dp
        val Full: Dp = 9999.dp
    }

    object Type {
        private val Sans = FontFamily.SansSerif
        private val Mono = FontFamily.Monospace

        val Display = TextStyle(fontFamily = Sans, fontSize = 20.sp, lineHeight = 26.sp, fontWeight = FontWeight.SemiBold)
        val Title = TextStyle(fontFamily = Sans, fontSize = 14.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold)
        val Ui = TextStyle(fontFamily = Sans, fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Normal)
        val UiStrong = TextStyle(fontFamily = Sans, fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Medium)
        val Meta = TextStyle(fontFamily = Sans, fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Normal)
        val MetaStrong = TextStyle(fontFamily = Sans, fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium)
        val Prose = TextStyle(fontFamily = Sans, fontSize = 14.sp, lineHeight = 22.sp, fontWeight = FontWeight.Normal)
        val Input = TextStyle(fontFamily = Mono, fontSize = 13.sp, lineHeight = 22.sp, fontWeight = FontWeight.Normal)
        val Code = TextStyle(fontFamily = Mono, fontSize = 12.sp, lineHeight = 18.sp, fontWeight = FontWeight.Normal)
        val CodeSm = TextStyle(fontFamily = Mono, fontSize = 11.sp, lineHeight = 18.sp, fontWeight = FontWeight.Normal)
        val Recap = TextStyle(fontFamily = Sans, fontSize = 13.sp, lineHeight = 20.sp, fontWeight = FontWeight.Normal)
        val CodeLarge = TextStyle(fontFamily = Mono, fontSize = 20.sp, lineHeight = 26.sp, fontWeight = FontWeight.Medium, letterSpacing = 1.6.sp)
    }
}

data class Fonts(val sans: FontFamily, val mono: FontFamily)

val LocalFonts = staticCompositionLocalOf { Fonts(FontFamily.SansSerif, FontFamily.Monospace) }

val LocalReducedMotion = staticCompositionLocalOf { false }

@Composable
fun TextStyle.themed(): TextStyle {
    val fonts = LocalFonts.current

    return when (fontFamily) {
        FontFamily.SansSerif -> copy(fontFamily = fonts.sans)
        FontFamily.Monospace -> copy(fontFamily = fonts.mono)
        else -> this
    }
}

val LocalOpenList = staticCompositionLocalOf<(() -> Unit)?> { null }

val LocalTouch = staticCompositionLocalOf { false }

val LocalPalette = staticCompositionLocalOf { Tokens.Dark }

object MyTheme {
    val colors: Palette
        @Composable get() = LocalPalette.current
}

data class Appearance(val dark: Boolean, val accent: Accent, val accents: List<Accent>, val onAccent: (Accent) -> Unit)

val LocalAppearance = staticCompositionLocalOf { Appearance(true, Accent.Neutral, emptyList()) {} }

@Composable
fun MyceliumTheme(dark: Boolean = isSystemInDarkTheme(), accent: Accent = Accent.Neutral, systemAccent: Color? = null, content: @Composable () -> Unit) {
    val fonts = Fonts(
        sans = FontFamily(
            Font(Res.font.ibmplexsans_regular, FontWeight.Normal),
            Font(Res.font.ibmplexsans_medium, FontWeight.Medium),
            Font(Res.font.ibmplexsans_semibold, FontWeight.SemiBold),
        ),
        mono = FontFamily(
            Font(Res.font.ibmplexmono_regular, FontWeight.Normal),
            Font(Res.font.ibmplexmono_medium, FontWeight.Medium),
        ),
    )

    CompositionLocalProvider(LocalPalette provides palette(dark, accent, systemAccent), LocalFonts provides fonts, content = content)
}
