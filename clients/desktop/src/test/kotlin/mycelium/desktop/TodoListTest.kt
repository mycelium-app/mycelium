package mycelium.desktop

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
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
import kotlin.test.assertNull

private val Entries = listOf(
    PlanEntry("Check the memory on pinning @immich/ui", PlanStatus.Done),
    PlanEntry("Pin @immich/ui to 0.31.2", PlanStatus.Done),
    PlanEntry("Run the tests across the workspace", PlanStatus.Running),
    PlanEntry("Commit the change", PlanStatus.Pending, "Use a conventional commit and mention the pin."),
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

    private var saved: Pair<List<PlanEntry>, Boolean>? = null

    private fun showEditable() {
        rule.setContent { MyceliumTheme(true) { TodoList(Entries, "c1", onSave = { todos, tellAgent -> saved = todos to tellAgent }) } }
        rule.onNodeWithContentDescription("To-do list, 3 of 4").performClick()
    }

    @Test
    fun anItemWithDetailsOpensToShowThem() {
        show()
        rule.onNodeWithContentDescription("To-do list, 3 of 4").performClick()

        rule.onNodeWithText("Use a conventional commit and mention the pin.").assertDoesNotExist()
        rule.onNodeWithContentDescription("Pending: Commit the change").performClick()

        rule.onNodeWithText("Use a conventional commit and mention the pin.").assertIsDisplayed()
        rule.onNodeWithContentDescription("Pending: Commit the change").assert(SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Expanded"))
    }

    @Test
    fun anEditedItemIsSavedAndTheAgentIsTold() {
        showEditable()

        rule.onNodeWithContentDescription("Edit the to-do list").performClick()
        rule.onNodeWithContentDescription("Item 4").performTextReplacement("Commit and push the change")
        rule.onNodeWithContentDescription("Save and tell the agent").performClick()

        assertEquals("Commit and push the change", saved?.first?.get(3)?.content)
        assertEquals("Use a conventional commit and mention the pin.", saved?.first?.get(3)?.description)
        assertEquals(true, saved?.second)
    }

    @Test
    fun aDismissedItemIsSavedWithoutTellingTheAgent() {
        showEditable()

        rule.onNodeWithContentDescription("Edit the to-do list").performClick()
        rule.onNodeWithContentDescription("Dismiss item 1").performClick()
        rule.onNodeWithText("1 change").assertIsDisplayed()
        rule.onNodeWithContentDescription("More ways to save").performClick()
        rule.onNodeWithText("Save without telling the agent").performClick()

        assertEquals(Entries.drop(1), saved?.first)
        assertEquals(false, saved?.second)
    }

    @Test
    fun anAddedItemTakesItsTextAndDetails() {
        showEditable()

        rule.onNodeWithContentDescription("Edit the to-do list").performClick()
        rule.onNodeWithText("Add item").performClick()
        rule.onNodeWithContentDescription("Item 5").performTextInput("Ask Sam to review")
        rule.onNodeWithContentDescription("Details of item 5").performTextInput("Sam knows the backups packages.")
        rule.onNodeWithContentDescription("Save and tell the agent").performClick()

        assertEquals(PlanEntry("Ask Sam to review", PlanStatus.Pending, "Sam knows the backups packages."), saved?.first?.last())
    }

    @Test
    fun anEmptyItemIsNotSavedAndSaysHowToFixIt() {
        showEditable()

        rule.onNodeWithContentDescription("Edit the to-do list").performClick()
        rule.onNodeWithText("Add item").performClick()
        rule.onNodeWithContentDescription("Save and tell the agent").performClick()

        assertNull(saved)
        rule.onNodeWithText("Enter what to do, or dismiss item 5").assertIsDisplayed()
    }

    @Test
    fun cancellingDropsTheDraft() {
        showEditable()

        rule.onNodeWithContentDescription("Edit the to-do list").performClick()
        rule.onNodeWithContentDescription("Dismiss item 1").performClick()
        rule.onNodeWithText("Cancel").performClick()

        assertNull(saved)
        rule.onNodeWithContentDescription("Done: Check the memory on pinning @immich/ui").assertIsDisplayed()
    }

    @Test
    fun aListThatCannotBeSavedOffersNoEdit() {
        show()
        rule.onNodeWithContentDescription("To-do list, 3 of 4").performClick()

        rule.onNodeWithContentDescription("Edit the to-do list").assertDoesNotExist()
    }
}
