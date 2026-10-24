package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.unit.dp
import mycelium.api.NodeSummary
import mycelium.api.Project
import mycelium.ui.MyButton
import mycelium.ui.MyceliumTheme
import mycelium.ui.ProjectMenu
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

class ProjectMenuTest {
    @get:Rule
    val rule = createComposeRule()

    private var picked: String? = null

    private fun open() {
        val projects = (0 until 100).map { Project("p$it", if (it == 99) "remote" else "local", "Project $it", "/work/repository-$it", createdAt = "2026-10-24T00:00:00Z") }
        val nodes = listOf("local" to "Laptop", "remote" to "Builder").associate { (id, name) ->
            id to NodeSummary(id = id, name = name, hostname = name, os = "linux", arch = "x86_64", version = "0.7.0", online = true, draining = false)
        }
        rule.setContent {
            var shown by remember { mutableStateOf(false) }

            MyceliumTheme(true) {
                Box(Modifier.width(400.dp)) {
                    MyButton({ shown = true }, text = "Choose project")

                    if (shown) {
                        ProjectMenu(projects, nodes, null, "Start in", onDismiss = { shown = false }) {
                            picked = it
                            shown = false
                        }
                    }
                }
            }
        }
        rule.onNodeWithText("Choose project").performClick()
    }

    @Test
    fun aLargeProjectListKeepsTheFolderChoiceVisibleAndFindsAProjectByName() {
        open()
        rule.onNodeWithText("A folder").assertIsDisplayed()
        rule.onNodeWithContentDescription("Search projects").performTextReplacement("PROJECT 99")
        rule.onNodeWithText("Project 99").performClick()
        assertEquals("p99", picked)
    }

    @Test
    fun projectsCanBeFoundByTheirPath() {
        open()
        rule.onNodeWithContentDescription("Search projects").performTextReplacement("repository-99")
        rule.onNodeWithText("Project 99").performClick()
        assertEquals("p99", picked)
    }

    @Test
    fun projectsCanBeFoundByTheirNode() {
        open()
        rule.onNodeWithContentDescription("Search projects").performTextReplacement("builder")
        rule.onNodeWithText("Project 99").performClick()
        assertEquals("p99", picked)
    }
}
