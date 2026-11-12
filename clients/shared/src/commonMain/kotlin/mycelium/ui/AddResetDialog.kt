package mycelium.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlin.time.Clock
import mycelium.api.AddManualResetRequest
import mycelium.state.Store
import mycelium.state.agentName

private const val FullReset = "Full reset"

private const val ModelReset = "Model reset"

private const val KindProblem = "Select a kind of reset"

private const val ModelProblem = "Enter the model the reset is for"

private const val RealDateProblem = "Enter a real date, like 27 11 2026"

private val DaysInMonth = listOf(31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31)

private fun dateProblem(day: String, month: String, year: String, today: String): String? {
    val missing = listOf("day" to day, "month" to month, "year" to year).filter { it.second.isBlank() }.map { it.first }

    if (missing.size == 3) {
        return "Enter the date the reset expires"
    }

    if (missing.isNotEmpty()) {
        return "Expiry date must include a ${missing.joinToString(" and ")}"
    }

    val dayNumber = day.toIntOrNull()
    val monthNumber = month.toIntOrNull()
    val yearNumber = year.toIntOrNull()
    val real = dayNumber != null && monthNumber != null && yearNumber != null && yearNumber in 1000..9999 && monthNumber in 1..12 && dayNumber in 1..DaysInMonth[monthNumber - 1]
    val leapDay = monthNumber == 2 && dayNumber == 29 && yearNumber != null && !(yearNumber % 4 == 0 && (yearNumber % 100 != 0 || yearNumber % 400 == 0))

    if (!real || leapDay) {
        return RealDateProblem
    }

    return if (isoDate(dayNumber!!, monthNumber!!, yearNumber!!) <= today) "Expiry date must be in the future" else null
}

private fun isoDate(day: Int, month: Int, year: Int) = "${year.toString().padStart(4, '0')}-${month.toString().padStart(2, '0')}-${day.toString().padStart(2, '0')}"

@Composable
fun AddResetDialog(store: Store, agent: String, account: String, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    var kind by remember(account) { mutableStateOf<String?>(null) }
    var model by remember(account) { mutableStateOf("") }
    var day by remember(account) { mutableStateOf("") }
    var month by remember(account) { mutableStateOf("") }
    var year by remember(account) { mutableStateOf("") }
    var kindProblem by remember(account) { mutableStateOf<String?>(null) }
    var modelProblem by remember(account) { mutableStateOf<String?>(null) }
    var expiryProblem by remember(account) { mutableStateOf<String?>(null) }
    var submissions by remember(account) { mutableStateOf(0) }
    val summaryFocus = remember { FocusRequester() }
    val kindFocus = remember { FocusRequester() }
    val modelFocus = remember { FocusRequester() }
    val dayFocus = remember { FocusRequester() }
    val runner = rememberAction(account)
    val problems = listOfNotNull(kindProblem?.let { it to kindFocus }, modelProblem?.let { it to modelFocus }, expiryProblem?.let { it to dayFocus })

    LaunchedEffect(submissions) {
        if (submissions > 0 && problems.isNotEmpty()) {
            runCatching { summaryFocus.requestFocus() }
        }
    }

    fun add() {
        val now = Clock.System.now()
        val today = (platform?.localDateTime(now.toEpochMilliseconds()) ?: now.toString()).substringBefore('T')

        kindProblem = KindProblem.takeIf { kind == null }
        modelProblem = ModelProblem.takeIf { kind == ModelReset && model.isBlank() }
        expiryProblem = dateProblem(day.trim(), month.trim(), year.trim(), today)
        submissions++

        if (kindProblem != null || modelProblem != null || expiryProblem != null) {
            return
        }

        val request = AddManualResetRequest(agent, account, if (kind == ModelReset) "model" else "full", model.trim().takeIf { kind == ModelReset }, isoDate(day.trim().toInt(), month.trim().toInt(), year.trim().toInt()))

        runner {
            store.api.addManualReset(request)
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Label("Add a reset for $account, ${agentName(agent)}", Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
            MyButton(onDismiss, icon = MyIcon.Close, description = "Close", muted = true)
        }

        if (problems.isNotEmpty()) {
            Column(
                Modifier.fillMaxWidth().border(Tokens.Size.Hairline, colors.statusFailed, ControlShape).padding(Tokens.Space.S3).focusRequester(summaryFocus).focusable().semantics { liveRegion = LiveRegionMode.Assertive },
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1),
            ) {
                Label("There is a problem", Modifier.semantics { heading() }, Tokens.Type.UiStrong, colors.statusFailed)

                for ((problem, focus) in problems) {
                    Label(problem, Modifier.clickable(role = Role.Button) { runCatching { focus.requestFocus() } }, Tokens.Type.Ui, colors.statusFailed, decoration = LabelDecoration.Underline)
                }
            }
        }

        Label("Copy a reset from Claude's settings so Usage counts it. Mycelium never contacts Anthropic about it.", style = Tokens.Type.Meta, color = colors.textMuted)

        Column(Modifier.focusRequester(kindFocus).focusable(), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Kind", style = Tokens.Type.MetaStrong, color = colors.textMuted)
            kindProblem?.let { Label(it, style = Tokens.Type.MetaStrong, color = colors.statusFailed) }
            Segmented(listOf(FullReset, ModelReset), kind.orEmpty(), { kind = it }, groupLabel = "Kind")
        }

        if (kind == ModelReset) {
            TextInput(model, { model = it }, label = "Model", hint = "As Claude's settings name it, for example Fable", error = modelProblem, errorAbove = true, onSubmit = ::add, focusRequester = modelFocus)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Expiry date", style = Tokens.Type.MetaStrong, color = colors.textMuted)
            Label("For example, 27 11 2026", style = Tokens.Type.Meta, color = colors.textMuted)
            expiryProblem?.let { Label(it, style = Tokens.Type.MetaStrong, color = colors.statusFailed) }

            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
                TextInput(day, { day = it.filter(Char::isDigit) }, label = "Day", modifier = Modifier.width(72.dp), invalid = expiryProblem != null, onSubmit = ::add, focusRequester = dayFocus)
                TextInput(month, { month = it.filter(Char::isDigit) }, label = "Month", modifier = Modifier.width(72.dp), invalid = expiryProblem != null, onSubmit = ::add)
                TextInput(year, { year = it.filter(Char::isDigit) }, label = "Year", modifier = Modifier.width(96.dp), invalid = expiryProblem != null, onSubmit = ::add)
            }
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::add, text = "Add reset", variant = ButtonVariant.Primary)
        }
    }
}
