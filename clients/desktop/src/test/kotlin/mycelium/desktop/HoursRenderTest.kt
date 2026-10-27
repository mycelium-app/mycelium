package mycelium.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import mycelium.api.Hours
import mycelium.api.Me
import mycelium.state.Store
import mycelium.ui.GroupsSettings
import mycelium.ui.MyTheme
import mycelium.ui.OpenOutOfHoursDialog
import mycelium.ui.Route
import mycelium.ui.Shell
import mycelium.ui.TimeAndFocusSettings
import mycelium.ui.WellnessScreen
import kotlin.test.Test
import kotlin.test.assertTrue

class HoursRenderTest {
    private val work = hoursGroupJson("g-work", "work", closedAllWeek())
    private val homelab = hoursGroupJson("g-homelab", "homelab")
    private val personal = hoursGroupJson("g-personal", "personal")
    private val conversations = listOf(hoursConversationJson("c1", "Bump the UI kit to 0.31.2", "g-work", "working"), hoursConversationJson("c2", "Tidy the dashboards", "g-homelab"))

    private fun storeFor(server: HoursServer, selected: String): Pair<Store, HoursPlatform> {
        val platform = HoursPlatform(selected)

        return server.store(CoroutineScope(Dispatchers.Unconfined), platform) to platform
    }

    private fun shell(name: String, server: HoursServer, selected: String) {
        val (store, platform) = storeFor(server, selected)
        val file = renderAt(name, true, false) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1")) }

        assertTrue(file.length() > 5_000)
    }

    private fun page(name: String, server: HoursServer, content: @androidx.compose.runtime.Composable (Store) -> Unit) {
        val (store, _) = storeFor(server, "all")
        val file = renderAt(name, true, false) { Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent).padding(24.dp)) { content(store) } }

        assertTrue(file.length() > 5_000)
    }

    @Test
    fun theWindDownBannerRendersAboveAnOpenConversation() {
        val closing = """{"always_open":false,"days":${hoursDaysJson(closed = false, opens = clockIn(-5), closes = clockIn(12))}}"""

        shell("hours-1.13-wind-down", HoursServer(listOf(hoursGroupJson("g-work", "work", closing), homelab), conversations), "g-work")
    }

    @Test
    fun theClosedGroupScreenRendersWhenTheSelectedGroupCloses() {
        val closedAfterHours = """{"always_open":false,"days":${hoursDaysJson(closed = false, opens = clockIn(-240), closes = clockIn(-60))}}"""
        val server = HoursServer(listOf(hoursGroupJson("g-work", "work"), homelab, personal), conversations)

        server.savedHoursResponse = hoursGroupJson("g-work", "work", closedAfterHours)

        val (store, platform) = storeFor(server, "g-work")
        val file = renderAt("hours-1.14-group-closed", true, false, interact = { runBlocking { store.saveGroupHours("g-work", Hours(alwaysOpen = false, days = emptyList())) } }) {
            Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Chat("c1"))
        }

        assertTrue(file.length() > 5_000)
    }

    @Test
    fun theMyceliumClosedScreenRenders() {
        val closed = WellnessSettingsJson.replace(Regex("\"closed\":(true|false),\"opens\":\"09:00\",\"closes\":\"1[78]:00\""), "\"closed\":false,\"opens\":\"${clockIn(120)}\",\"closes\":\"${clockIn(240)}\"")

        shell("hours-1.15-mycelium-closed", HoursServer(listOf(work, homelab), conversations, closed), "all")
    }

    @Test
    fun theOpenedOutOfHoursBannerRenders() {
        shell("hours-1.18-opened", HoursServer(listOf(hoursGroupJson("g-work", "work", closedAllWeek(), openedUntil = isoIn(14)), homelab), conversations), "g-work")
    }

    @Test
    fun theOpenOutOfHoursDialogRenders() {
        val server = HoursServer(listOf(work, homelab), conversations)
        val (store, platform) = storeFor(server, "all")
        val closed = store.hoursView().groups.values.filter { !it.open }.map { it.group }
        val file = renderAt("hours-1.17-dialog", true, false) {
            Box(Modifier.fillMaxSize().background(MyTheme.colors.bgContent)) { OpenOutOfHoursDialog(store, platform, closed, "g-work", {}, {}) }
        }

        assertTrue(file.length() > 5_000)
    }

    @Test
    fun theHoursSettingsAndGroupPagesRender() {
        val server = HoursServer(listOf(work, homelab, personal), conversations)

        page("hours-9.2-time-and-focus", HoursServer(listOf(work, homelab, personal), conversations, WellnessSettingsJson)) { TimeAndFocusSettings(it) }
        page("hours-9.21-group-hours", server) { GroupsSettings(it, "g-work", {}, "Hours") }
        page("hours-9.8-groups", server) { GroupsSettings(it, null, {}) }
    }

    @Test
    fun theOverviewListsWhatWasOpenedOutOfHours() {
        val server = HoursServer(listOf(work, homelab), conversations)
        val (store, platform) = storeFor(server, "all")
        val file = renderAt("hours-8.1-overview", true, false) { WellnessScreen(store, platform) }

        assertTrue(file.length() > 5_000)
    }
}
