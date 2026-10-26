package mycelium.desktop

import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.unit.Density
import androidx.compose.runtime.CompositionLocalProvider
import kotlinx.coroutines.runBlocking
import mycelium.state.Store
import mycelium.ui.LocalTouch
import mycelium.ui.MyceliumTheme
import mycelium.ui.WellnessScreen
import org.jetbrains.skia.EncodedImageFormat
import org.junit.Rule
import java.io.File
import kotlin.test.Test
import kotlin.test.assertTrue

class SkillsRenderTest {
    @get:Rule
    val scopes = TestScopes()

    @Test
    fun theOverviewAndSkillsTabRenderAtTwoWidths() {
        for (width in listOf(900, 1440)) {
            for (tab in listOf("overview", "skills")) {
                val store = Store(LearningRequests().api(), scopes.create())

                runBlocking { store.wellness.load(); store.learning.loadSkills() }

                val scene = ImageComposeScene(width, 900, Density(1f)) { MyceliumTheme(true) { WellnessScreen(store, OpeningPlatform(), startOn = tab) } }

                repeat(8) { scene.render(it * 16_000_000L) }

                val out = File("build/screens/wellness-$tab-$width.png")
                out.parentFile.mkdirs()
                out.writeBytes(scene.render(200_000_000L).encodeToData(EncodedImageFormat.PNG)!!.bytes)
                scene.close()
                assertTrue(out.length() > 2_000)
            }
        }
    }
}
