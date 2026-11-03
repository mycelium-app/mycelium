package mycelium

import mycelium.api.HarnessModel
import mycelium.api.HarnessModels
import mycelium.api.NodeSummary
import mycelium.state.modelMenu
import mycelium.state.reportedEffort
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlin.test.assertNull
import kotlin.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals

private val Now = Instant.parse("2026-10-27T12:00:00Z")

private fun node(vararg reports: HarnessModels) = NodeSummary("n", "atlas-box", hostname = "atlas", os = "linux", arch = "x86_64", version = "0.4.0", online = true, draining = false, harnessModels = reports.toList())

private val Claude = HarnessModels("claude", null, listOf(HarnessModel("opus", "Opus 5.5"), HarnessModel("sonnet", "Sonnet 5.5")), "opus", "2026-10-27T10:00:00Z")

class HarnessModelsTest {
    @Test
    fun theMenuNamesTheNodeAndHowOldTheReportIsAndTicksTheDefaultThatItResolvesTo() {
        val menu = modelMenu(node(Claude), "claude", null, Now)

        assertEquals("Claude Code on atlas-box, 2 hours ago", menu.title)
        assertEquals(listOf("", "opus", "sonnet"), menu.choices.map { it.id })
        assertEquals(listOf("Default model", "Opus 5.5", "Sonnet 5.5"), menu.choices.map { it.name })
        assertEquals("Opus 5.5", menu.choices.first().description)
    }

    @Test
    fun aNodeThatHasReportedNothingOffersOnlyTheDefaultModelAndSaysWhenTheListAppears() {
        val menu = modelMenu(node(), "claude", null, Now)

        assertEquals("atlas-box has not reported its models yet. They appear after its first session.", menu.title)
        assertEquals(listOf("Default model"), menu.choices.map { it.name })
    }

    @Test
    fun aListReportedForAnotherAgentIsNotOffered() {
        assertEquals(listOf("Default model"), modelMenu(node(Claude), "codex", null, Now).choices.map { it.name })
    }

    @Test
    fun effortUsesOnlyTheSettingsReportedForTheChosenAccountAndModel() {
        val settings = Json.parseToJsonElement("""[{"id":"reasoning_level","name":"Effort","category":"thought_level","currentValue":"balanced","options":[{"value":"brief","name":"Brief"},{"value":"balanced","name":"Balanced"}]}]""").jsonArray
        val reported = node(Claude.copy(account = "sam@example.org", configOptions = settings))
        val effort = reportedEffort(reported, "claude", "sam@example.org", "opus")

        assertEquals("reasoning_level", effort?.id)
        assertEquals("balanced", effort?.current)
        assertEquals(listOf("brief", "balanced"), effort?.choices?.map { it.id })
        assertEquals(effort, reportedEffort(reported, "claude", "sam@example.org", ""))
        assertNull(reportedEffort(reported, "claude", "sam@example.org", "sonnet"))
        assertNull(reportedEffort(reported, "claude", "alex@example.org", "opus"))
        assertNull(reportedEffort(reported, "codex", "sam@example.org", "opus"))
        assertNull(reportedEffort(node(), "claude", null, ""))
    }

}
