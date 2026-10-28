package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import org.cef.browser.CefBrowser
import org.junit.Rule
import java.lang.reflect.Proxy
import kotlin.test.Test
import kotlin.test.assertEquals

class BrowserBoundsTest {
    @get:Rule
    val rule = createComposeRule()

    private val cefBrowser = Proxy.newProxyInstance(CefBrowser::class.java.classLoader, arrayOf(CefBrowser::class.java)) { _, _, _ -> null } as CefBrowser

    @Test
    fun theBrowserFollowsItsContainerWhenTheSidebarResizesIt() {
        val host = OffscreenBrowser()
        var panelWidth by mutableStateOf(400.dp)

        rule.setContent {
            Box(Modifier.width(panelWidth)) {
                Box(Modifier.fillMaxSize().then(rememberBrowserBounds(host, 1.0) { }))
            }
        }
        rule.mainClock.advanceTimeBy(100)
        rule.waitForIdle()

        assertEquals(400, host.getViewRect(cefBrowser).width)

        panelWidth = 250.dp
        rule.mainClock.advanceTimeBy(100)
        rule.waitForIdle()

        assertEquals(250, host.getViewRect(cefBrowser).width)

        panelWidth = 600.dp
        rule.mainClock.advanceTimeBy(100)
        rule.waitForIdle()

        assertEquals(600, host.getViewRect(cefBrowser).width)
    }
}
