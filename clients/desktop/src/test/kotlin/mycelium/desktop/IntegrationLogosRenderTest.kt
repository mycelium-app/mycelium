package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import mycelium.ui.BrandLogo
import mycelium.ui.MyTheme
import kotlin.test.Test
import kotlin.test.assertTrue

class IntegrationLogosRenderTest {
    @Test
    fun everyBrandLogoRendersInBothThemes() {
        for (dark in listOf(true, false)) {
            val sizes = listOf("lastfm", "gitlab", "github", "zulip", "stoat").map { brand ->
                renderAt("logo-$brand", dark, false) {
                    Row(Modifier.background(MyTheme.colors.bgContent).padding(16.dp), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        BrandLogo(brand, 64.dp)
                    }
                }.length()
            }

            assertTrue(sizes.all { it > 2_000 }, "$sizes")
        }
    }
}
