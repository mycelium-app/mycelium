package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import mycelium.api.Conversation
import mycelium.state.hasSession
import mycelium.state.hasWorkspace

enum class BarGroup { Tools, Status, Pickers, Usage, Turn }

enum class BarControl(val group: BarGroup) {
    Attach(BarGroup.Tools),
    Actions(BarGroup.Tools),
    Dictate(BarGroup.Tools),
    TeachMe(BarGroup.Tools),
    VoiceMode(BarGroup.Tools),
    Goal(BarGroup.Tools),
    Stopping(BarGroup.Status),
    Model(BarGroup.Pickers),
    Mode(BarGroup.Pickers),
    Settings(BarGroup.Pickers),
    Usage(BarGroup.Usage),
    Stop(BarGroup.Turn),
    Send(BarGroup.Turn),
}

data class HarnessReport(
    val voice: Boolean = false,
    val voiceMode: Boolean = false,
    val inVoiceMode: Boolean = false,
    val teachMe: Boolean = false,
    val working: Boolean = false,
    val stopping: Boolean = false,
    val models: Boolean = false,
    val modes: Boolean = false,
    val settings: Boolean = false,
)

data class BarSubject(
    val starting: Boolean = false,
    val choosesMode: Boolean = false,
    val assistant: Boolean = false,
    val goal: Boolean = false,
    val configurable: Boolean = false,
) {
    companion object {
        val NewConversation = BarSubject(starting = true, choosesMode = true)
        val NewChat = BarSubject(starting = true)
        val NewAssistantSession = BarSubject(assistant = true)

        fun of(conversation: Conversation): BarSubject {
            val assistant = conversation.assistantId != null

            return BarSubject(
                assistant = assistant,
                goal = conversation.hasWorkspace && !assistant,
                configurable = conversation.hasSession && (conversation.hasWorkspace || !assistant),
            )
        }
    }
}

fun barControls(subject: BarSubject, harness: HarnessReport): List<BarControl> {
    val starting = subject.starting

    return listOfNotNull(
        BarControl.Attach,
        BarControl.Actions.takeIf { subject.assistant },
        BarControl.Dictate.takeIf { harness.voice && !harness.inVoiceMode && !starting },
        BarControl.TeachMe.takeIf { harness.teachMe && !starting },
        BarControl.VoiceMode.takeIf { harness.voiceMode && !starting },
        BarControl.Goal.takeIf { subject.goal },
        BarControl.Stopping.takeIf { harness.working && harness.stopping && !starting },
        BarControl.Mode.takeIf { harness.modes && subject.choosesMode },
        BarControl.Model,
        BarControl.Mode.takeIf { harness.modes && subject.configurable },
        BarControl.Settings.takeIf { harness.settings && subject.configurable },
        BarControl.Usage.takeIf { !starting },
        BarControl.Stop.takeIf { harness.working && !harness.stopping && !starting },
        BarControl.Send,
    )
}

fun List<BarControl>.inGroups(vararg groups: BarGroup): List<BarControl> = filter { it.group in groups }

@Composable
fun ComposerBar(controls: List<BarControl>, modifier: Modifier = Modifier, render: @Composable (BarControl) -> Unit) {
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1), verticalAlignment = Alignment.CenterVertically) {
        controls.inGroups(BarGroup.Tools).forEach { render(it) }

        Row(
            Modifier.weight(1f).horizontalScrollFaded(),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S1, Alignment.End),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            controls.inGroups(BarGroup.Status, BarGroup.Pickers).forEach { render(it) }
        }

        controls.inGroups(BarGroup.Usage, BarGroup.Turn).forEach { render(it) }
    }
}
