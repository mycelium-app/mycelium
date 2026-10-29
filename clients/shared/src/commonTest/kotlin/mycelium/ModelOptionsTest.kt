package mycelium

import mycelium.api.AgentInfo
import mycelium.api.HarnessModel
import mycelium.api.HarnessModels
import mycelium.api.ModelChoice
import mycelium.api.NodeSummary
import mycelium.state.anyModelHint
import mycelium.state.favouriteOptions
import mycelium.state.isFavourite
import mycelium.state.matching
import mycelium.state.modelOptions
import mycelium.state.withFavourite
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

private fun node(id: String, name: String, vararg reports: HarnessModels) =
    NodeSummary(
        id,
        name,
        hostname = name,
        os = "linux",
        arch = "x86_64",
        version = "0.4.0",
        online = true,
        draining = false,
        agents = listOf(AgentInfo("claude", available = true), AgentInfo("codex", available = true)),
        harnessModels = reports.toList(),
    )

private val Claude = HarnessModels("claude", null, listOf(HarnessModel("opus", "Claude Opus 5.5"), HarnessModel("sonnet", "Claude Sonnet 5.5")), "opus", "2026-10-27T10:00:00Z")

private val Codex = HarnessModels("codex", null, listOf(HarnessModel("sol", "GPT-6.1 Sol")), "sol", "2026-10-27T10:00:00Z")

private val Laptop = node("n1", "laptop", Claude)

private val Orion = node("n2", "orion", Claude, Codex)

class ModelOptionsTest {
    @Test
    fun aModelTheChosenNodeLacksIsOfferedButGreyedWithWhereItLives() {
        val options = modelOptions(listOf(Laptop, Orion), Laptop, emptyList())
        val sol = options.first { it.name == "GPT-6.1 Sol" }

        assertFalse(sol.available)
        assertEquals("On orion", sol.where)
        assertTrue(options.first { it.name == "Claude Opus 5.5" }.available)
    }

    @Test
    fun aModelBothNodesHaveIsListedOnceAndAvailable() {
        val options = modelOptions(listOf(Laptop, Orion), Laptop, emptyList()).filter { it.agent == "claude" }

        assertEquals(listOf("Claude Opus 5.5", "Claude Sonnet 5.5"), options.map { it.name })
    }

    @Test
    fun withoutAChosenNodeEveryModelIsAvailable() {
        assertTrue(modelOptions(listOf(Laptop, Orion), null, emptyList()).all { it.available })
    }

    @Test
    fun favouritesKeepTheirSavedOrderNotTheOrderOfTheMenu() {
        val options = modelOptions(listOf(Laptop, Orion), Laptop, emptyList())
        val saved = listOf(ModelChoice(agent = "codex", model = "sol"), ModelChoice(agent = "claude", model = "opus"))
        val favourites = saved.favouriteOptions(options, emptyList())

        assertEquals(listOf("GPT-6.1 Sol", "Claude Opus 5.5"), favourites.map { it.name })
        assertEquals("GPT-6.1 Sol, then Claude Opus 5.5", anyModelHint(favourites))
    }

    @Test
    fun aStarAddsAModelToTheEndAndAnotherPressRemovesIt() {
        val options = modelOptions(listOf(Laptop), Laptop, emptyList())
        val opus = options.first { it.model == "opus" }
        val sonnet = options.first { it.model == "sonnet" }
        val starred = emptyList<ModelChoice>().withFavourite(opus, true).withFavourite(sonnet, true)

        assertEquals(listOf("opus", "sonnet"), starred.map { it.model })
        assertTrue(starred.isFavourite(opus))
        assertEquals(listOf("sonnet"), starred.withFavourite(opus, false).map { it.model })
    }

    @Test
    fun aFavouriteSavedWithAnAccountIsStillTheSameModelWhenUnstarred() {
        val opus = modelOptions(listOf(Laptop), Laptop, emptyList()).first { it.model == "opus" }
        val saved = listOf(ModelChoice(agent = "claude", model = "opus", account = "sam@example.com"))

        assertTrue(saved.isFavourite(opus))
        assertEquals(emptyList(), saved.withFavourite(opus, false))
    }

    @Test
    fun searchMatchesTheModelNameOrTheHarness() {
        val options = modelOptions(listOf(Laptop, Orion), Laptop, emptyList())

        assertEquals(listOf("GPT-6.1 Sol"), options.matching("sol").map { it.name })
        assertEquals(listOf("Claude Opus 5.5", "Claude Sonnet 5.5"), options.matching("claude code").map { it.name })
        assertEquals(emptyList(), options.matching("nothing like this"))
    }
}
