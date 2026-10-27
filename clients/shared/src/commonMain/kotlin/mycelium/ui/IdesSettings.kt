package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.Platform
import mycelium.state.Choice
import mycelium.state.Ide
import mycelium.state.IdePreference
import mycelium.state.IdeLauncher
import mycelium.state.absoluteInFolder
import mycelium.state.IdeSettings
import mycelium.api.NodeSummary
import mycelium.state.ideSettings
import mycelium.state.stored

val LocalIdes = staticCompositionLocalOf<IdeLauncher?> { null }

class IdeTarget(val node: NodeSummary, val folder: String)

val LocalIdeTarget = staticCompositionLocalOf<IdeTarget?> { null }

@Composable
fun OpenInIdeButton(path: String, line: Int? = null) {
    val target = LocalIdeTarget.current

    if (target != null) {
        OpenInIdeButton(target.node, absoluteInFolder(target.folder, path), line)
    }
}

@Composable
fun OpenInIdeButton(node: NodeSummary?, path: String, line: Int? = null, labelled: Boolean = false) {
    val ides = LocalIdes.current
    val ide = node?.let { ides?.main(it) }

    if (ides != null && node != null && ide != null) {
        MyButton({ ides.open(node, ide, path, line) }, text = if (labelled) "Open in ${ide.label}" else null, icon = if (labelled) null else MyIcon.Code, description = "Open ${path.substringAfterLast('/')}${line?.let { " at line $it" }.orEmpty()} in ${ide.label}", variant = ButtonVariant.Outline, size = ButtonSize.Sm, tooltip = "Open in ${ide.label}")
    }
}

@Composable
fun IdesSettings(platform: Platform) {
    val colors = MyTheme.colors
    var saved by remember { mutableStateOf(ideSettings(platform.loadPreference(IdePreference))) }
    var draft by remember { mutableStateOf(saved) }

    fun change(ide: Ide, changed: (mycelium.state.IdeChoice) -> mycelium.state.IdeChoice) {
        draft = draft.copy(choices = Ide.entries.map { if (it == ide) changed(draft.choiceFor(it)) else draft.choiceFor(it) })
    }

    Column(Modifier.widthIn(max = Tokens.Size.Page), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S3)) {
        Label("IDEs", Modifier.semantics { heading() }, Tokens.Type.Title)
        Label("Saved on this computer. A folder on another node opens through the IDE's SSH remote, by way of Mycelium, so the node needs no open port. Leave a command empty to use the one shown.", style = Tokens.Type.Meta, color = colors.textMuted)

        Table(
            listOf(Col("Offer", 64.dp, preferred = 64.dp, rigid = true), Col("IDE", 110.dp, rigid = true), Col("Command or path", 200.dp, weight = 1f)),
            Ide.entries.size,
            { row, column ->
                val ide = Ide.entries[row]
                val choice = draft.choiceFor(ide)

                when (column) {
                    0 -> TableCell.Slot("Offer") { Checkbox(choice.offered, "Offer ${ide.label}", { change(ide) { it.copy(offered = !it.offered) } }, checkedState = "On", uncheckedState = "Off") }
                    1 -> TableCell.Text(ide.label)
                    else -> TableCell.Slot("/opt/jetbrains/idea/bin/idea.sh", Tokens.Space.S4) { TextInput(choice.command, { value -> change(ide) { it.copy(command = value) } }, label = "Command or path for ${ide.label}", placeholder = ide.defaultCommand, mono = true, showLabel = false) }
                }
            },
            headerCell = { if (it == 0) TableCell.Text("") else TableCell.Text(listOf("", "IDE", "Command or path")[it], Tokens.Type.MetaStrong) },
        )

        Column(Modifier.widthIn(max = 280.dp), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Default IDE")
            OptionMenu("Default IDE", Ide.entries.map { Choice(it.id, it.label) }, draft.default, { draft = draft.copy(default = it) })
        }

        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton({ saved = draft; platform.savePreference(IdePreference, draft.stored()) }, text = "Save", variant = ButtonVariant.Outline)
            MyButton({ draft = saved }, text = "Cancel")
        }
    }
}
