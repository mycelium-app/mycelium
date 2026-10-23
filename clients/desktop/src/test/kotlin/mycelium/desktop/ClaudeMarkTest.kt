package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import mycelium.ui.Icon
import mycelium.ui.MyIcon
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class ClaudeMarkTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun claudesMarkIsASolidShapeNotAnOutline() {
        rule.setContent {
            Box(Modifier.testTag("mark").background(Color.Black)) {
                Icon(MyIcon.ClaudeMark, Color.White, 96.dp)
            }
        }

        val pixels = rule.onNodeWithTag("mark").captureToImage().toPixelMap()

        assertEquals(Color.White, pixels[pixels.width / 2, pixels.height / 2])
        assertEquals(Color.White, pixels[pixels.width * 3 / 8, pixels.height / 6])
    }
}
