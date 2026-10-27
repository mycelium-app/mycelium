package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import mycelium.api.Walkthrough
import mycelium.api.WalkthroughAnswer
import mycelium.api.WalkthroughCheck
import mycelium.api.WalkthroughAnswerRequest
import mycelium.state.Store

const val CheckStepTitle = "Check your understanding"

@Composable
fun WalkthroughCheckStep(store: Store, walkthrough: Walkthrough, onBackToStep: (Int) -> Unit) {
    var skipped by remember(walkthrough.id) { mutableStateOf(false) }
    var answers by remember(walkthrough.id) { mutableStateOf(emptyMap<Int, WalkthroughAnswer>()) }

    LaunchedEffect(walkthrough.id) {
        var stored = emptyList<WalkthroughAnswer>()
        attempt { stored = store.api.walkthroughAnswers(walkthrough.id) }

        answers = stored.associateBy { it.step } + answers
    }

    Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            Label(CheckStepTitle, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)

            if (!skipped) {
                MyButton({ skipped = true }, text = "Skip", description = "Skip the questions", size = ButtonSize.Sm, muted = true)
            }
        }

        if (skipped) {
            Label("Skipped. Nothing was recorded.", color = MyTheme.colors.textMuted)
            MyButton({ skipped = false }, text = "Answer the questions", variant = ButtonVariant.Outline, size = ButtonSize.Sm)

            return@Column
        }

        for (check in walkthrough.checks.sortedBy { it.step }) {
            CheckQuestion(
                check = check,
                answer = answers[check.step],
                onCheck = { choice -> answers = answers + (check.step to store.api.answerWalkthroughCheck(walkthrough.id, check.step, WalkthroughAnswerRequest(choice))) },
                onBack = { onBackToStep(check.step - 1) },
            )
        }
    }
}

@Composable
private fun CheckQuestion(check: WalkthroughCheck, answer: WalkthroughAnswer?, onCheck: suspend (Int) -> Unit, onBack: () -> Unit) {
    val colors = MyTheme.colors
    var chosen by remember(check.step) { mutableStateOf(answer?.choice) }
    var error by remember(check.step) { mutableStateOf<String?>(null) }
    val attempt = rememberAttempt { error = it }

    LaunchedEffect(answer?.choice) {
        if (chosen == null) {
            chosen = answer?.choice
        }
    }

    Column(Modifier.fillMaxWidth().semantics(mergeDescendants = false) {}, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
        Label("Step ${check.step}: ${check.question}", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

        for ((position, choice) in check.choices.withIndex()) {
            MenuItem(choice, { chosen = position; error = null }, checked = chosen == position)
        }

        error?.let { Label(it, style = Tokens.Type.MetaStrong, color = colors.statusFailed) }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
            MyButton(
                {
                    val picked = chosen

                    if (picked == null) {
                        error = "Choose an answer to check it."
                    } else {
                        attempt { onCheck(picked) }
                    }
                },
                text = "Check",
                description = "Check your answer to step ${check.step}",
                variant = ButtonVariant.Outline,
                size = ButtonSize.Sm,
            )
        }

        if (answer != null) {
            Column(Modifier.semantics { liveRegion = LiveRegionMode.Polite }, verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
                    Icon(if (answer.correct) MyIcon.Check else MyIcon.Alert, if (answer.correct) colors.statusDone else colors.statusAttention, Tokens.Size.IconSm)
                    Label(if (answer.correct) "Correct" else "Not quite", style = Tokens.Type.UiStrong)
                }

                if (answer.explanation.isNotBlank()) {
                    Label(answer.explanation, color = colors.textMuted)
                }

                MyButton(onBack, text = "Back to step ${check.step}", variant = ButtonVariant.Ghost, size = ButtonSize.Sm)
            }
        }
    }
}
