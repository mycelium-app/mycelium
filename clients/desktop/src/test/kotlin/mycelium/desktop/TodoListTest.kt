package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import mycelium.state.PlanEntry
import mycelium.state.PlanStatus
import mycelium.ui.MyceliumTheme
import mycelium.ui.TodoList
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private val Entries = listOf(
    PlanEntry("Check the memory on pinning @immich/ui", PlanStatus.Done),
    PlanEntry("Pin @immich/ui to 0.31.2", PlanStatus.Done),
    PlanEntry("Run the tests across the workspace", PlanStatus.Running),
    PlanEntry("Commit the change", PlanStatus.Pending),
)

class TodoListTest {
    @get:Rule
    val rule = createComposeRule()

    private fun show() {
        rule.setContent { MyceliumTheme(true) { TodoList(Entries, "c1") } }
    }

    @Test
    fun collapsedItShowsTheCurrentItemAndItsPlaceInTheList() {
        show()

        rule.onNodeWithContentDescription("To-do list, 3 of 4").assertIsDisplayed()
        rule.onNodeWithContentDescription("To-do list, 3 of 4").assertTextContains("Run the tests across the workspace")
        assertEquals(0, rule.onAllNodesWithContentDescription("Done: Pin @immich/ui to 0.31.2").fetchSemanticsNodes().size)
    }

    @Test
    fun togglingTheRowExpandsTheChecklistWithEachItemsStatus() {
        show()

        rule.onNodeWithContentDescription("To-do list, 3 of 4").performClick()

        rule.onNodeWithContentDescription("Done: Pin @immich/ui to 0.31.2").assertIsDisplayed()
        rule.onNodeWithContentDescription("In progress: Run the tests across the workspace").assertIsDisplayed()
        rule.onNodeWithContentDescription("Pending: Commit the change").assertIsDisplayed()

        rule.onNodeWithContentDescription("To-do list, 3 of 4").performClick()

        assertEquals(0, rule.onAllNodesWithContentDescription("Pending: Commit the change").fetchSemanticsNodes().size)
    }
}
