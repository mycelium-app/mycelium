package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.unit.dp
import mycelium.api.Walkthrough
import mycelium.api.WalkthroughAnswer
import mycelium.api.WalkthroughCheck
import mycelium.api.WalkthroughAnswerRequest
import mycelium.state.Store

const val CheckStepTitle = "Check your understanding"

private val QuestionMeasure = 600.dp

@Composable
fun WalkthroughCheckStep(store: Store, walkthrough: Walkthrough, onBackToStep: (Int) -> Unit) {
    var skipped by remember(walkthrough.id) { mutableStateOf(false) }
    var answers by remember(walkthrough.id) { mutableStateOf(emptyMap<Int, WalkthroughAnswer>()) }

    LaunchedEffect(walkthrough.id) {
        var stored = emptyList<WalkthroughAnswer>()
        attempt { stored = store.api.walkthroughAnswers(walkthrough.id) }

        answers = stored.associateBy { it.step } + answers
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4)) {
        Label(CheckStepTitle, Modifier.semantics { heading() }, Tokens.Type.Display)

        if (skipped) {
            Label("Skipped. Nothing was recorded.", color = MyTheme.colors.textMuted)
            MyButton({ skipped = false }, text = "Answer the questions", variant = ButtonVariant.Outline, size = ButtonSize.Sm)

            return@Column
        }

        val checks = walkthrough.checks.sortedBy { it.step }

        for (check in checks) {
            CheckQuestion(
                check = check,
                answer = answers[check.step],
                onCheck = { choice -> answers = answers + (check.step to store.api.answerWalkthroughCheck(walkthrough.id, check.step, WalkthroughAnswerRequest(choice))) },
                onBack = { onBackToStep(check.step - 1) },
                onSkip = { skipped = true }.takeIf { check == checks.last() },
            )
        }
    }
}

@Composable
private fun CheckQuestion(check: WalkthroughCheck, answer: WalkthroughAnswer?, onCheck: suspend (Int) -> Unit, onBack: () -> Unit, onSkip: (() -> Unit)?) {
    val colors = MyTheme.colors
    var chosen by remember(check.step) { mutableStateOf(answer?.choice) }
    val runner = rememberAction(check.step)

    LaunchedEffect(answer?.choice) {
        if (chosen == null) {
            chosen = answer?.choice
        }
    }

    Column(
        Modifier.widthIn(max = QuestionMeasure).fillMaxWidth().background(colors.bgPanel, BlockShape).border(Tokens.Size.Hairline, colors.border, BlockShape).padding(Tokens.Space.S4),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3),
    ) {
        Column(Modifier.selectableGroup().semantics { contentDescription = check.question }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            Column(Modifier.padding(bottom = Tokens.Space.S1).semantics(mergeDescendants = true) { heading() }) {
                Label("Step ${check.step}", style = Tokens.Type.Meta, color = colors.textMuted)
                Label(check.question, style = Tokens.Type.Title)
            }

            for ((position, choice) in check.choices.withIndex()) {
                RadioRow(chosen == position, { chosen = position; runner.failure = null }, bordered = true) {
                    Label(choice, Modifier.weight(1f))
                }
            }
        }

        runner.failure?.let { Label(it, style = Tokens.Type.MetaStrong, color = colors.statusFailed) }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            MyButton(
                {
                    val picked = chosen

                    if (picked == null) {
                        runner.failure = "Choose an answer to check it."
                    } else {
                        runner { onCheck(picked) }
                    }
                },
                text = "Check",
                description = "Check your answer to step ${check.step}",
                variant = ButtonVariant.Primary,
                size = ButtonSize.Sm,
            )

            if (onSkip != null) {
                MyButton(onSkip, text = "Skip the questions", size = ButtonSize.Sm, muted = true)
            }
        }

        if (answer != null) {
            Hairline()

            Column(Modifier.semantics { liveRegion = LiveRegionMode.Polite }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5), verticalAlignment = Alignment.CenterVertically) {
                    Icon(if (answer.correct) MyIcon.Check else MyIcon.Alert, if (answer.correct) colors.statusDone else colors.statusAttention, Tokens.Size.IconSm)
                    Label(if (answer.correct) "Correct" else "Not quite", style = Tokens.Type.UiStrong)
                }

                if (answer.explanation.isNotBlank()) {
                    Label(answer.explanation, color = colors.textMuted)
                }

                val back = "Back to step ${check.step}"
                val linkStyle = TextLinkStyles(SpanStyle(color = colors.statusWorking, textDecoration = TextDecoration.Underline))

                BasicText(buildAnnotatedString { withLink(LinkAnnotation.Clickable(back, linkStyle) { onBack() }) { append(back) } }, style = Tokens.Type.Ui)
            }
        }
    }
}
