package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.unit.dp
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.serialization.json.Json
import kotlinx.serialization.serializer
import mycelium.api.Event
import mycelium.api.Task
import mycelium.api.TaskStatuses
import mycelium.state.buildThread
import mycelium.ui.LocalQuestionAnswers
import mycelium.ui.MyTheme
import mycelium.ui.MyceliumTheme
import mycelium.ui.QuestionAnswers
import mycelium.ui.ThreadView
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals

private const val QuestionId = "66666666-6666-6666-6666-666666666666"

private fun completedCall(id: String, seq: Long, title: String, input: String, output: String): List<Event> {
    val text = Json.encodeToString(serializer<String>(), output)

    return listOf(
        Event("c1", seq, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"$id","title":"$title","kind":"other","status":"pending","rawInput":{}}}""")),
        Event("c1", seq + 1, "acp_update", Json.parseToJsonElement("""{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"$id","status":"completed","rawInput":$input,"content":[{"type":"content","content":{"type":"text","text":$text}}]}}""")),
    )
}

private fun asked(options: String = """["Yes","No"]""") = completedCall("t5", 1, "mcp__mycelium__ask_question", """{"question":"Should the walkthrough include test files?","options":$options}""", "Question $QuestionId asked. End your turn: the answer arrives here as a new message.")

private fun question(status: String = TaskStatuses.Question, answer: String? = null) =
    Task(QuestionId, "n1", "claude", "/work/app", title = "Should the walkthrough include test files?", createdAt = "2026-10-22T10:00:00Z", status = status, options = listOf("Yes", "No"), answer = answer, sourceId = "c1")

class AskQuestionCardTest {
    @get:Rule
    val rule = createComposeRule()

    private val answers = CopyOnWriteArrayList<String>()
    private var task by mutableStateOf<Task?>(question())

    private fun show(events: List<Event>) {
        val items = buildThread(events)
        rule.setContent {
            MyceliumTheme(true) {
                val provided = QuestionAnswers(
                    tasks = listOfNotNull(task).associateBy { it.id },
                    refresh = {},
                    answer = { asking, text ->
                        answers += text
                        task = asking.copy(status = TaskStatuses.Done, answer = text)
                    },
                )

                CompositionLocalProvider(LocalQuestionAnswers provides provided) {
                    Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(items, onAnswer = { _, _ -> }, onStopWorkflow = {}) }
                }
            }
        }
    }

    @Test
    fun aQuestionWithOptionsOffersEachAsAButtonAndPickingOneSendsIt() {
        show(asked())

        rule.onNodeWithText("Waiting for your answer").assertExists()
        rule.onNodeWithContentDescription("Answer Should the walkthrough include test files?: No").performClick()

        assertEquals(listOf("No"), answers)
    }

    @Test
    fun aFreeAnswerIsSentFromTheCardAndTheCardThenShowsItAsAnswered() {
        show(asked())

        rule.onNodeWithContentDescription("Answer").performTextInput("Only the changed ones")
        rule.onNodeWithText("Send answer").performClick()

        assertEquals(listOf("Only the changed ones"), answers)
        rule.waitForIdle()
        rule.onNodeWithText("Answered").assertExists()
        rule.onNodeWithText("Answered: Only the changed ones").assertExists()
        rule.onNodeWithText("Send answer").assertDoesNotExist()
    }

    @Test
    fun aQuestionAnsweredElsewhereShowsItsAnswerAndNoControls() {
        task = question(TaskStatuses.Done, "Yes")
        show(asked())

        rule.onNodeWithText("Answered").assertExists()
        rule.onNodeWithText("Answered: Yes").assertExists()
        rule.onNodeWithText("Send answer").assertDoesNotExist()
    }

    @Test
    fun anEmptyAnswerIsRefusedWithWhatToDo() {
        show(asked("[]"))

        rule.onNodeWithText("Send answer").performClick()

        rule.onNodeWithText("Enter an answer to send.").assertExists()
        assertEquals(emptyList(), answers)
    }

    @Test
    fun theQuestionCardRendersOpenAndAnsweredNextToAScheduleCard() {
        val events = asked() + completedCall("t6", 3, "mcp__mycelium__propose_schedule", """{"prompt":"Check CI and summarise failures","preset":"weekdays at 20:00"}""", "Schedule 77777777-7777-7777-7777-777777777777 proposed (0 20 * * 1-5, Europe/London). It waits for the person to confirm and will not run until they do.")

        for ((name, current) in listOf("question-open" to question(), "question-answered" to question(TaskStatuses.Done, "Only the changed ones"))) {
            val file = renderAt(name, true, false) {
                CompositionLocalProvider(LocalQuestionAnswers provides QuestionAnswers(mapOf(QuestionId to current), {}, { _, _ -> })) {
                    Box(Modifier.width(640.dp).background(MyTheme.colors.bgContent).padding(16.dp)) { ThreadView(buildThread(events), onAnswer = { _, _ -> }, onStopWorkflow = {}) }
                }
            }

            assertEquals(true, file.length() > 0)
        }
    }
}
