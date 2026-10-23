package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import kotlinx.serialization.json.JsonObject
import mycelium.state.ChatToolName
import mycelium.state.ThreadItem
import mycelium.state.ToolSearchName
import mycelium.state.ToolStatus
import mycelium.state.readableArguments

private const val OutputLines = 6

@Composable
internal fun ArgumentLines(input: JsonObject?) {
    val colors = MyTheme.colors

    for ((name, value) in readableArguments(input)) {
        Row(Modifier.semantics(mergeDescendants = true) { contentDescription = "$name: $value" }, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            Label(name, style = Tokens.Type.Code, color = colors.textMuted)
            Label(value, Modifier.weight(1f), Tokens.Type.Code, maxLines = 4)
        }
    }
}

@Composable
internal fun ChatToolCard(call: ThreadItem.ToolCall, name: ChatToolName?) {
    val colors = MyTheme.colors
    val searching = call.title == ToolSearchName
    val heading = if (searching) "Searched for tools" else name?.heading ?: call.title
    val state = when (call.status) {
        ToolStatus.Failed -> "Failed"
        ToolStatus.Done -> "Done"
        ToolStatus.Running -> "Running"
        ToolStatus.Pending -> "Waiting"
    }

    Card(Modifier.semantics { contentDescription = "$heading${name?.let { ", ${it.server}" }.orEmpty()}, ${state.lowercase()}" }) {
        CardHead {
            Icon(if (searching) MyIcon.Search else MyIcon.Sliders, colors.textMuted, Tokens.Size.IconSm)
            Label(heading, style = Tokens.Type.UiStrong, maxLines = 1)

            if (name != null) {
                Label(name.server, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
            }

            Box(Modifier.weight(1f))

            when (call.status) {
                ToolStatus.Failed -> StatusMark("Failed", Tone.Failed, MyIcon.Failed)
                ToolStatus.Done -> StatusMark("Done", Tone.Muted, MyIcon.Check)
                ToolStatus.Running -> StatusMark("Running", Tone.Working, MyIcon.Working)
                ToolStatus.Pending -> StatusMark("Waiting", Tone.Muted, MyIcon.Clock)
            }
        }

        val output = call.output?.trim()?.takeIf { it.isNotEmpty() && !searching }

        if (call.status != ToolStatus.Pending) {
            Column(Modifier.padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                ArgumentLines(call.input)

                if (output != null) {
                    Label(output, style = Tokens.Type.Ui, color = if (call.status == ToolStatus.Failed) colors.statusFailed else colors.textMuted, maxLines = OutputLines)
                }
            }
        }
    }
}
