package mycelium.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

private val Seam = 1.dp

class PanelDemand {
    var width by mutableStateOf(0.dp)
}

val LocalPanelDemand = staticCompositionLocalOf<PanelDemand?> { null }

fun sidebarDocks(window: Dp, sidebar: Dp, demand: Dp): Boolean = window >= sidebar + Seam * 2 + Tokens.Size.ChatMin + demand

fun panelTakesOver(area: Dp): Boolean = area < Tokens.Size.ChatMin + Tokens.Size.SidePanelMin + Tokens.Size.Rail + Seam

fun panelDemand(saved: Dp, open: Boolean, browser: Boolean): Dp = when {
    !open -> Tokens.Size.Rail
    browser -> maxOf(saved, Tokens.Size.SidePanelBrowser) + Tokens.Size.Rail
    else -> saved + Tokens.Size.Rail
}

fun shownPanelWidth(saved: Dp, area: Dp, browser: Boolean): Dp {
    val wanted = if (browser) maxOf(saved, Tokens.Size.SidePanelBrowser) else saved

    if (area == Dp.Unspecified) {
        return wanted
    }

    return minOf(wanted, area - Tokens.Size.ChatMin - Tokens.Size.Rail - Seam).coerceAtLeast(Tokens.Size.SidePanelMin)
}
