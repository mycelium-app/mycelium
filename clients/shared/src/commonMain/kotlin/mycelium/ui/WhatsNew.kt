package mycelium.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import mycelium.ChangelogText
import mycelium.ClientVersion
import mycelium.Platform
import mycelium.state.LastLaunchedVersionPreference
import mycelium.state.Release
import mycelium.state.WhatsNewSettingsCount
import mycelium.state.parseChangelog
import mycelium.state.releasesSince
import mycelium.state.versionParts

@Composable
fun WhatsNewOnLaunch(platform: Platform, running: String = ClientVersion, changelog: String = ChangelogText) {
    val unseen = remember { releasesSince(parseChangelog(changelog), platform.loadPreference(LastLaunchedVersionPreference), running) }
    var open by remember { mutableStateOf(unseen.isNotEmpty()) }

    LaunchedEffect(Unit) {
        if (unseen.isEmpty() && versionParts(running) != null) {
            platform.savePreference(LastLaunchedVersionPreference, running)
        }
    }

    if (open) {
        WhatsNewDialog(unseen) {
            platform.savePreference(LastLaunchedVersionPreference, running)
            open = false
        }
    }
}

@Composable
fun RecentReleasesDialog(onClose: () -> Unit, changelog: String = ChangelogText) {
    val recent = remember { parseChangelog(changelog).take(WhatsNewSettingsCount) }

    WhatsNewDialog(recent, onClose)
}

@Composable
private fun WhatsNewDialog(releases: List<Release>, onClose: () -> Unit) {
    val colors = MyTheme.colors
    val closeFocus = remember { FocusRequester() }

    LaunchedEffect(Unit) {
        closeFocus.requestFocus()
    }

    Dialog(onClose) {
        Label("What's new", Modifier.semantics { heading() }, Tokens.Type.Title)

        Column(
            Modifier.heightIn(max = 420.dp).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.S4),
        ) {
            for (release in releases) {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.S1_5)) {
                    Label(release.title, Modifier.semantics { heading() }, Tokens.Type.UiStrong)

                    for (note in release.notes) {
                        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.S2)) {
                            Box(Modifier.padding(top = Tokens.Space.S1_5).size(Tokens.Space.S1).background(colors.textMuted, CircleShape))
                            Label(note, Modifier.weight(1f), color = colors.text)
                        }
                    }
                }
            }
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
            MyButton(onClose, Modifier.focusRequester(closeFocus), text = "Close", variant = ButtonVariant.Outline)
        }
    }
}
