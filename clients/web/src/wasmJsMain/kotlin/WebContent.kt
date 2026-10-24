import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity

@JsFun("""(id, url, left, top, width, height) => {
    let frame = document.getElementById(id);
    if (!frame) {
        frame = document.createElement('iframe');
        frame.id = id;
        frame.style.position = 'absolute';
        frame.style.border = '0';
        frame.title = 'Artefact';
        document.body.appendChild(frame);
    }
    const canvas = document.querySelector('canvas');
    const offset = canvas ? canvas.getBoundingClientRect() : { left: 0, top: 0 };
    frame.style.left = (offset.left + left) + 'px';
    frame.style.top = (offset.top + top) + 'px';
    frame.style.width = width + 'px';
    frame.style.height = height + 'px';
    if (frame.getAttribute('src') !== url) { frame.setAttribute('src', url); }
}""")
private external fun placeFrame(id: String, url: String, left: Double, top: Double, width: Double, height: Double)

@JsFun("(id) => { const frame = document.getElementById(id); if (frame) frame.remove(); }")
private external fun removeFrame(id: String)

private var frameCount = 0

@Composable
fun IframeWebContent(url: String, modifier: Modifier) {
    val id = remember { "artefact-frame-${frameCount++}" }
    val density = LocalDensity.current.density

    DisposableEffect(id) {
        onDispose { removeFrame(id) }
    }

    Box(
        modifier.onGloballyPositioned { coordinates ->
            val position = coordinates.positionInWindow()

            placeFrame(
                id,
                url,
                (position.x / density).toDouble(),
                (position.y / density).toDouble(),
                (coordinates.size.width / density).toDouble(),
                (coordinates.size.height / density).toDouble(),
            )
        },
    )
}
