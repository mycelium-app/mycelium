package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import mycelium.api.Goal
import mycelium.api.GoalState
import mycelium.state.PlanEntry
import mycelium.state.PlanStatus
import mycelium.ui.GoalAndTodoRegion
import mycelium.ui.GoalControls
import mycelium.ui.MyceliumTheme
import mycelium.ui.TodoList
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private val Entries = listOf(
    PlanEntry("Check the memory on pinning @immich/ui", PlanStatus.Done),
    PlanEntry("Pin @immich/ui to 0.31.2", PlanStatus.Done),
    PlanEntry("Run the tests across the workspace", PlanStatus.Running),
    PlanEntry("Commit the change", PlanStatus.Pending),
)

private val RunningGoal = Goal(conversationId = "c1", text = "Every test passes on main", state = GoalState.Active)

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

    @Test
    fun theGoalAndTheTodoListAreOneRegionLabelledWithBoth() {
        rule.setContent { MyceliumTheme(true) { GoalAndTodoRegion(GoalControls(RunningGoal, { _, _ -> }, {}, {}, {}, {}), Entries, "c1") } }

        rule.onNodeWithContentDescription("Goal and to-do list").assertIsDisplayed()
        rule.onNodeWithContentDescription("Goal").assertDoesNotExist()
        rule.onNodeWithContentDescription("To-do list").assertDoesNotExist()
    }

    @Test
    fun aTodoListAloneKeepsItsOwnRegionLabel() {
        rule.setContent { MyceliumTheme(true) { GoalAndTodoRegion(null, Entries, "c1") } }

        rule.onNodeWithContentDescription("To-do list").assertIsDisplayed()
        rule.onNodeWithContentDescription("Goal and to-do list").assertDoesNotExist()
    }

    @Test
    fun theCurrentItemIsMarkedAsTheCurrentStep() {
        show()

        rule.onNodeWithContentDescription("To-do list, 3 of 4").performClick()

        rule.onNodeWithContentDescription("In progress: Run the tests across the workspace").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Current step"))
    }
}
