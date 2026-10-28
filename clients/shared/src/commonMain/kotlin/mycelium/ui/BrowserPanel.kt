package mycelium.ui

import androidx.compose.foundation.background
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

val LocalOutlinePage = staticCompositionLocalOf<OutlinePage?> { null }

val LocalOutlineOpener = staticCompositionLocalOf<((OutlinePage) -> Unit)?> { null }

@Composable
fun BrowserPanel(platform: Platform, url: String, title: String, onClose: () -> Unit, modifier: Modifier = Modifier, reading: PageReading? = null) {
    val colors = MyTheme.colors

    Column(modifier.fillMaxSize().background(colors.bgPanel).semantics { contentDescription = title }) {
        Bar {
            PanelLeading()
            Icon(MyIcon.Globe, colors.textMuted, Tokens.Size.IconMd)
            Label(title, Modifier.weight(1f), Tokens.Type.Title, maxLines = 1)
            MyButton({ platform.openUrl(url) }, icon = MyIcon.Globe, description = "Open in browser", size = ButtonSize.Sm, tooltip = "Open in browser")
            MyButton(onClose, icon = MyIcon.Close, description = "Close $title", size = ButtonSize.Sm, tooltip = "Close")
        }

        Label(url.substringAfter("://"), Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S1), Tokens.Type.Meta, colors.textMuted, maxLines = 1)

        Hairline()

        platform.WebContent(url, null, Modifier.weight(1f).fillMaxWidth(), reading)
    }
}
