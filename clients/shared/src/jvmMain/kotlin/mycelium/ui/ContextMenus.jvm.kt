package mycelium.ui

import androidx.compose.foundation.DefaultContextMenuRepresentation
import androidx.compose.foundation.LocalContextMenuRepresentation
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider

@Composable
actual fun ThemedContextMenus(content: @Composable () -> Unit) {
    val colors = MyTheme.colors

    CompositionLocalProvider(
        LocalContextMenuRepresentation provides DefaultContextMenuRepresentation(backgroundColor = colors.bgRaised, textColor = colors.text, itemHoverColor = colors.bgHover),
        content = content,
    )
}
