package mycelium.desktop

import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.test.ComposeUiTest
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.onLast
import java.io.File

private val ShotsFolder = File("/tmp/mycelium-build-flag-adopt/shots")

@OptIn(ExperimentalTestApi::class)
internal fun ComposeUiTest.saveShot(name: String) {
    waitForIdle()
    val bitmap = onAllNodes(isRoot()).onLast().captureToImage().asSkiaBitmap()
    val bytes = org.jetbrains.skia.Image.makeFromBitmap(bitmap).encodeToData(org.jetbrains.skia.EncodedImageFormat.PNG)!!.bytes

    ShotsFolder.mkdirs()
    File(ShotsFolder, "$name.png").writeBytes(bytes)
}
