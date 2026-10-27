package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.contentDescription
import mycelium.PageReading
import mycelium.Platform
import mycelium.state.OutlinePage

val LocalOutlineOpener = staticCompositionLocalOf<((OutlinePage) -> Unit)?> { null }

@Composable
fun BrowserPanel(platform: Platform, url: String, title: String, caption: String, onClose: () -> Unit, modifier: Modifier = Modifier, reading: PageReading? = null) {
    val colors = MyTheme.colors

    Column(modifier.fillMaxSize().background(colors.bgPanel).semantics { contentDescription = title }) {
        Bar {
            Icon(MyIcon.Globe, colors.textMuted, Tokens.Size.IconMd)
            Label(title, Modifier.weight(1f), Tokens.Type.Title, maxLines = 1)
            MyButton({ platform.openUrl(url) }, text = "Open in browser", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
            MyButton(onClose, text = "Close", variant = ButtonVariant.Outline, size = ButtonSize.Sm, description = "Close $title")
        }

        Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
            Label(url.substringAfter("://"), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            Label(caption, style = Tokens.Type.Meta, color = colors.textMuted)
        }

        Hairline()

        platform.WebContent(url, null, Modifier.weight(1f).fillMaxWidth(), reading)
    }
}
