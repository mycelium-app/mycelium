package mycelium.desktop

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onChildren
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import mycelium.ui.MyceliumTheme
import mycelium.ui.ResizeHandle
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class ResizeHandleTest {
    @get:Rule
    val rule = createComposeRule()

    @Test
    fun theArrowKeysMoveTheHandleOneStepEachWay() {
        val moves = mutableListOf<Dp>()
        rule.setContent { MyceliumTheme(true) { ResizeHandle("Resize the side panel") { moves += it } } }
        val handle = rule.onNodeWithContentDescription("Resize the side panel").onChildren().onFirst()

        handle.requestFocus()
        handle.performKeyInput { pressKey(Key.DirectionLeft) }
        handle.performKeyInput { pressKey(Key.DirectionRight) }

        assertEquals(listOf((-16).dp, 16.dp), moves)
    }
}
