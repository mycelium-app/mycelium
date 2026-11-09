package mycelium.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.api.ModelProvider
import mycelium.api.SpeechProvider
import mycelium.state.Choice
import mycelium.state.ColumnAlign
import mycelium.state.Store

const val SpeechAnchor = "speech"
const val NoSpeechToTextStatement = "No provider can turn speech into text. Add an OpenAI-compatible provider with a speech-to-text model, such as whisper-1."
const val NoTextToSpeechStatement = "Add a provider with a text-to-speech model to hear replies."

private val UsedForWidth = 152.dp

private val StackedBelow = 480.dp

private val ChangeColumn = Col("", 52.dp, rigid = true, align = ColumnAlign.End)

private const val SpeechToTextUse = "speech_to_text"

private const val OtherModels = "Other models"

private class SpeechUse(val id: String, val name: String, val recognisedTitle: String, val chosen: SpeechProvider?, val recognised: (ModelProvider) -> List<String>)

@Composable
fun SpeechSettings(store: Store, providers: List<ModelProvider>, onAddProvider: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MyTheme.colors
    val voice by store.voice.state.collectAsState()
    var changing by remember { mutableStateOf<String?>(null) }
    val openAiCompatible = providers.filter { it.kind == "openai_compatible" }
    val uses = listOf(
        SpeechUse(SpeechToTextUse, "Speech to text", "Speech-to-text models", voice.privacy.speechToTextProvider) { it.speechToTextModels },
        SpeechUse("text_to_speech", "Reading replies aloud", "Text-to-speech models", voice.privacy.textToSpeechProvider) { it.textToSpeechModels },
    )

    LaunchedEffect(providers) {
        store.voice.load()
    }

    BoxWithConstraints(modifier) {
        val narrow = maxWidth < StackedBelow

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Speech", Modifier.semantics { heading() }, Tokens.Type.UiStrong)

            Table(
                if (narrow) {
                    listOf(Col("Used for", 110.dp, weight = 1f), ChangeColumn)
                } else {
                    listOf(Col("Used for", UsedForWidth, rigid = true), Col("Provider", 90.dp, weight = 1f), Col("Model", 110.dp), ChangeColumn)
                },
                uses.size,
                { row, column ->
                    val use = uses[row]
                    val chosen = use.chosen
                    val change = TableCell.Slot("Change") {
                        MyButton({ changing = use.id }, text = "Change", description = "Change the provider for ${use.name.lowercase()}", size = ButtonSize.Sm, muted = true)
                    }

                    when {
                        narrow && column == 0 -> TableCell.Slot(use.name) {
                            Column(Modifier.padding(vertical = Tokens.Space.S1_5), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S0_5)) {
                                Label(use.name, style = Tokens.Type.UiStrong, maxLines = 1)
                                Label(chosen?.name.orEmpty(), style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1)
                                Label(chosen?.model.orEmpty(), style = Tokens.Type.Code, color = colors.textMuted, maxLines = 1)
                            }
                        }

                        narrow -> change
                        column == 0 -> TableCell.Text(use.name)
                        column == 1 -> TableCell.Slot(providers.firstOrNull { it.id == chosen?.providerId }?.baseUrl ?: chosen?.name) {
                            Column {
                                Label(chosen?.name.orEmpty(), maxLines = 1)
                                providers.firstOrNull { it.id == chosen?.providerId }?.baseUrl?.let { Label(it, style = Tokens.Type.Meta, color = colors.textMuted, maxLines = 1) }
                            }
                        }

                        column == 2 -> TableCell.Text(chosen?.model.orEmpty(), Tokens.Type.Code)
                        else -> change
                    }
                },
                rowSpan = { row ->
                    val use = uses[row]

                    if (use.chosen != null) {
                        null
                    } else {
                        @Composable {
                            val missing = @Composable {
                                if (use.id == SpeechToTextUse) {
                                    FailureBlock {
                                        Label(NoSpeechToTextStatement, style = Tokens.Type.Meta)
                                        FlowRow(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                                            MyButton(onAddProvider, text = "Add a provider", icon = MyIcon.Plus, description = "Add a provider for speech to text", variant = ButtonVariant.Outline, size = ButtonSize.Sm)

                                            if (openAiCompatible.isNotEmpty()) {
                                                MyButton({ changing = use.id }, text = "Choose a model", description = "Choose a speech-to-text model from your providers", size = ButtonSize.Sm, muted = true)
                                            }
                                        }
                                    }
                                } else {
                                    Label(NoTextToSpeechStatement, style = Tokens.Type.Meta, color = colors.textMuted)
                                }
                            }

                            if (narrow) {
                                Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.S3, vertical = Tokens.Space.S2), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                                    Label(use.name, style = Tokens.Type.UiStrong)
                                    missing()
                                }
                            } else {
                                Row(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.S2), verticalAlignment = Alignment.CenterVertically) {
                                    Box(Modifier.width(UsedForWidth + Tokens.Space.S3).padding(start = Tokens.Space.S3)) { Label(use.name) }
                                    Box(Modifier.weight(1f).padding(horizontal = Tokens.Space.S3)) { missing() }
                                }
                            }
                        }
                    }
                },
            )
        }
    }

    uses.firstOrNull { it.id == changing }?.let { use ->
        SpeechModelDialog(store, use, openAiCompatible) { changing = null }
    }
}

@Composable
private fun SpeechModelDialog(store: Store, use: SpeechUse, providers: List<ModelProvider>, onDismiss: () -> Unit) {
    val colors = MyTheme.colors
    val start = providers.firstOrNull { it.id == use.chosen?.providerId } ?: providers.firstOrNull { use.recognised(it).isNotEmpty() } ?: providers.firstOrNull()
    var providerId by remember { mutableStateOf(start?.id) }
    val provider = providers.firstOrNull { it.id == providerId }
    val recognised = provider?.let(use.recognised).orEmpty()
    val others = provider?.models.orEmpty().filter { it !in recognised }
    var model by remember { mutableStateOf(use.chosen?.takeIf { it.providerId == start?.id }?.model ?: recognised.firstOrNull()) }
    val runner = rememberAction()

    fun save() {
        val chosenProvider = provider ?: return
        val chosenModel = model

        if (chosenModel == null) {
            runner.failure = "Select a model for ${use.name.lowercase()}."

            return
        }

        runner {
            store.voice.chooseSpeechModel(use.id, chosenProvider.id, chosenModel)
            onDismiss()
        }
    }

    Dialog(onDismiss) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Label(use.name, Modifier.weight(1f).semantics { heading() }, Tokens.Type.Title)
            MyButton(onDismiss, icon = MyIcon.Close, description = "Close", size = ButtonSize.Sm, muted = true)
        }

        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
            Label("Provider")
            Row {
                OptionMenu("Provider", providers.map { Choice(it.id, it.name) }, providerId, { picked ->
                    providerId = picked
                    model = providers.firstOrNull { it.id == picked }?.let(use.recognised)?.firstOrNull()
                    runner.failure = null
                }, variant = ButtonVariant.Outline)
            }

            if (provider != null && provider.models.isEmpty()) {
                Label("${provider.name} offers no models. Select another provider, or refresh its models in Chat models.", style = Tokens.Type.Meta, color = colors.statusFailed)
            }
        }

        if (provider != null && provider.models.isNotEmpty()) {
            Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1)) {
                Label("Model")
                Row {
                    OptionMenu(
                        "Model",
                        recognised.map { Choice(it, it) } + others.map { Choice(it, it, group = OtherModels.takeIf { recognised.isNotEmpty() }) },
                        model,
                        { model = it; runner.failure = null },
                        title = if (recognised.isNotEmpty()) use.recognisedTitle else OtherModels,
                        variant = ButtonVariant.Outline,
                    )
                }
            }
        }

        runner.failure?.let { ErrorBanner(it, onDismiss = { runner.failure = null }) }

        FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2, Alignment.End), verticalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
            MyButton(onDismiss, text = "Cancel", muted = true)
            MyButton(::save, text = "Save", variant = ButtonVariant.Primary)
        }
    }
}
