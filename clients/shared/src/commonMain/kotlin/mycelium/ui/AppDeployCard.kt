package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import kotlinx.coroutines.delay
import mycelium.api.DeploymentProgress
import mycelium.api.DeploymentStep
import mycelium.state.ChatToolName
import mycelium.state.DeployCall
import mycelium.state.addressLabel
import mycelium.state.addressUrl
import mycelium.state.formatDuration

val LocalAppDeployments = compositionLocalOf<(suspend (String) -> DeploymentProgress)?> { null }

private const val DeployPollMillis = 2_000L

private fun progressWord(status: String): String = when (status) {
    "deployed" -> "Deployed"
    "failed" -> "Failed"
    "cancelled" -> "Cancelled"
    else -> "Deploying"
}

private fun stepIcon(state: String): MyIcon = when (state) {
    "done" -> MyIcon.Check
    "running" -> MyIcon.Working
    "failed" -> MyIcon.Failed
    else -> MyIcon.Clock
}

@Composable
internal fun AppDeployCard(call: DeployCall, name: ChatToolName?, fetch: suspend (String) -> DeploymentProgress) {
    val colors = MyTheme.colors
    val platform = LocalPlatform.current
    val openApp = LocalAppOpener.current
    var progress by remember(call.deploymentUuid) { mutableStateOf<DeploymentProgress?>(null) }
    var failure by remember(call.deploymentUuid) { mutableStateOf<String?>(null) }

    LaunchedEffect(call.deploymentUuid) {
        var deploying = true

        while (deploying) {
            failure = attempt {
                val next = fetch(call.deploymentUuid)
                progress = next
                deploying = next.status == "deploying"
            }

            if (failure != null) {
                deploying = false
            } else if (deploying) {
                delay(DeployPollMillis)
            }
        }
    }

    val current = progress
    val app = current?.appName?.ifBlank { null } ?: call.appName ?: "the app"
    val status = current?.status ?: "deploying"
    val address = current?.address ?: call.address

    Card(Modifier.semantics { contentDescription = "Deploying $app, ${progressWord(status).lowercase()}" }) {
        CardHead {
            Icon(MyIcon.AppWindow, colors.textMuted, Tokens.Size.IconSm)
            Label("Deploying $app", style = Tokens.Type.UiStrong, maxLines = 1)

            if (name != null) {
                Label(name.server, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            Box(Modifier.weight(1f))

            when (status) {
                "deployed" -> StatusMark("Deployed", Tone.Muted, MyIcon.Check)
                "failed", "cancelled" -> StatusMark(progressWord(status), Tone.Failed, MyIcon.Failed)
                else -> StatusMark("Deploying", Tone.Working, MyIcon.Working)
            }
        }

        Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
            for (step in current?.steps.orEmpty()) {
                StepRow(step)
            }

            failure?.let { Label(it, style = Tokens.Type.Meta, color = colors.statusFailed) }

            if (status == "deployed" && address != null) {
                Label("Deployed at ${addressLabel(address)}", Modifier.semantics { liveRegion = LiveRegionMode.Polite }, Tokens.Type.Ui)
            }

            if ((status == "failed" || status == "cancelled") && current != null) {
                Column(Modifier.semantics { liveRegion = LiveRegionMode.Assertive }) {
                    for (line in current.logTail.takeLast(TailLines)) {
                        Label(line, style = Tokens.Type.Code, color = colors.statusFailed)
                    }
                }
            }

            FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                val appUuid = current?.appUuid

                if (openApp != null && appUuid != null) {
                    MyButton({ openApp(appUuid, call.deploymentUuid) }, text = "Build log", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }

                if (address != null && platform != null) {
                    MyButton({ platform.openUrl(addressUrl(address)) }, text = "Open app", description = "Open ${addressLabel(address)}", variant = ButtonVariant.Outline, size = ButtonSize.Sm)
                }
            }
        }
    }
}

private const val TailLines = 6

@Composable
private fun StepRow(step: DeploymentStep) {
    val colors = MyTheme.colors
    val tint = when (step.state) {
        "done" -> colors.statusDone
        "running" -> colors.statusWorking
        "failed" -> colors.statusFailed
        else -> colors.textFaint
    }

    Row(
        Modifier.semantics(mergeDescendants = true) { contentDescription = "${step.label}, ${step.state}${step.durationSeconds?.let { ", ${formatDuration(it * 1000)}" }.orEmpty()}" },
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(stepIcon(step.state), tint, Tokens.Size.IconSm)
        Label(step.label, Modifier.weight(1f), color = if (step.state == "pending") colors.textMuted else colors.text)
        step.durationSeconds?.let { Label(formatDuration(it * 1000), style = Tokens.Type.Meta, color = colors.textMuted) }
    }
}
