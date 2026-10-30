package mycelium.desktop

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onLast
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.performTextInput
import mycelium.api.ModelChoice
import mycelium.api.MyceliumJson
import mycelium.api.NodeSummary
import mycelium.state.PickerAccount
import mycelium.state.ModelOption
import mycelium.ui.ModelPicker
import mycelium.ui.ModelPickerControls
import mycelium.state.UsageLimitNote
import mycelium.ui.LimitActions
import mycelium.ui.LocalTouch
import mycelium.ui.UsageLimitCard
import mycelium.ui.PickerRefusal
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val WillowAndAtlas = """[
{"id":"n1","name":"willow","hostname":"willow","os":"linux","arch":"x86_64","version":"v0.11.0","address":"10.0.0.5","online":true,"draining":false,"sessions":["c1"],
"agents":[{"agent":"claude","available":true},{"agent":"codex","available":true}],
"logins":[{"agent":"claude","account":"sam@example.com","method":"token","state":"signed_in","usage_left":18},{"agent":"codex","account":"codex@example.com","method":"token","state":"signed_in"}]},
{"id":"n2","name":"atlas","hostname":"atlas","os":"linux","arch":"x86_64","version":"v0.11.0","address":"10.0.0.6","online":true,"draining":false,"sessions":[],
"agents":[{"agent":"claude","available":true}],
"logins":[{"agent":"claude","account":"work@example.org","method":"token","state":"signed_in","usage_left":64}]}]"""

class ModelPickerTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.Collections.synchronizedList(mutableListOf<Pair<String, String>>())

    private val nodes = MyceliumJson.decodeFromString<List<NodeSummary>>(WillowAndAtlas)

    private val options = listOf(
        ModelOption("claude", null, "opus", "Claude Opus 5.5"),
        ModelOption("claude", null, "sonnet", "Claude Sonnet 5.5"),
        ModelOption("codex", null, "sol", "GPT-6.1 Sol", available = false, where = "On orion"),
        ModelOption(null, "p1", "llama3", "llama3"),
    )

    private fun row(node: NodeSummary, agent: String, account: String, usage: String, here: Boolean) = PickerAccount("on:${node.id}:$agent:$account", agent, account, node, here, usage)

    private fun showPicker(
        favourites: List<ModelChoice> = listOf(ModelChoice(agent = "claude", model = "opus")),
        onPickModel: (ModelOption) -> Unit = {},
        onFavourite: (ModelOption, Boolean) -> Unit = { _, _ -> },
        onPickAccount: (String?) -> Unit = {},
    ) {
        val willow = nodes.first { it.name == "willow" }
        val atlas = nodes.first { it.name == "atlas" }
        val accounts = listOf(row(atlas, "claude", "work@example.org", "64% left", false), row(willow, "claude", "sam@example.com", "18% left", true))

        rule.setContent {
            MyceliumTheme(true) {
                ModelPicker(
                    ModelPickerControls(
                        label = "Claude Opus 5.5",
                        options = options,
                        favourites = favourites,
                        providers = emptyList(),
                        chosen = ModelChoice(agent = "claude", model = "opus"),
                        onPickModel = onPickModel,
                        onPickAny = {},
                        onFavourite = onFavourite,
                        accounts = accounts,
                        onPickAccount = { picked, done -> onPickAccount(picked); done(null) },
                    ),
                )
            }
        }

        rule.onNodeWithContentDescription("Model: Claude Opus 5.5").performClick()
    }

    @Test
    fun theMenuOpensWithASearchFieldAnyModelFavouritesAndOneHeadingPerHarness() {
        showPicker()

        rule.onNodeWithContentDescription("Search models").assertIsDisplayed()
        rule.onNodeWithText("Any model").assertIsDisplayed()

        for (heading in listOf("Favourites", "Claude Code", "Codex", "Chat")) {
            rule.onAllNodes(hasScrollAction()).onLast().performScrollToNode(hasText(heading))
            rule.onAllNodesWithText(heading).onFirst().assertIsDisplayed()
        }
    }

    @Test
    fun aModelTheChosenNodeLacksIsGreyedAndSaysWhereItLives() {
        var picked: ModelOption? = null
        showPicker(onPickModel = { picked = it })

        rule.onAllNodes(hasScrollAction()).onLast().performScrollToNode(hasText("GPT-6.1 Sol"))
        rule.onNodeWithText("On orion").assertExists()
        rule.onNodeWithContentDescription("Codex: GPT-6.1 Sol, on orion").performClick()

        assertEquals(null, picked)
    }

    @Test
    fun aFavouriteListedTwiceStillOpensTheMenu() {
        showPicker(favourites = listOf(ModelChoice(agent = "claude"), ModelChoice(agent = "claude")))

        rule.onNodeWithContentDescription("Search models").assertIsDisplayed()
        rule.onNodeWithText("Favourites").assertIsDisplayed()
    }

    @Test
    fun choosingAModelReportsItAndClosesTheMenu() {
        var picked: ModelOption? = null
        showPicker(onPickModel = { picked = it })

        rule.onNodeWithContentDescription("Claude Code: Claude Sonnet 5.5").performClick()

        assertEquals("sonnet", picked?.model)
        assertEquals(0, rule.onAllNodesWithContentDescription("Search models").fetchSemanticsNodes().size)
    }

    @Test
    fun typingNarrowsTheListAndEnterPicksTheFirstAvailableMatch() {
        var picked: ModelOption? = null
        showPicker(onPickModel = { picked = it })

        rule.onNodeWithContentDescription("Search models").performTextInput("sonnet")

        assertEquals(0, rule.onAllNodesWithText("llama3").fetchSemanticsNodes().size)
        rule.onNodeWithContentDescription("Search models").performImeAction()

        assertEquals("sonnet", picked?.model)
    }

    @Test
    fun theStarAddsAndRemovesAFavourite() {
        val changes = mutableListOf<Pair<String, Boolean>>()
        showPicker(onFavourite = { option, on -> changes += option.model to on })

        rule.onNodeWithContentDescription("Add to favourites: Claude Sonnet 5.5").performClick()
        rule.onAllNodesWithContentDescription("Remove from favourites: Claude Opus 5.5").onFirst().performClick()

        assertEquals(listOf("sonnet" to true, "opus" to false), changes)
    }

    @Test
    fun theAccountSubmenuOffersAnyAccountAndEachAccountWithItsNodeAndUsage() {
        var picked: String? = "unset"
        showPicker(onPickAccount = { picked = it })

        rule.onNodeWithText("Account").performClick()

        rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").assertIsDisplayed()
        rule.onNodeWithContentDescription("Claude Code · sam@example.com on this device, 18% left").assertIsDisplayed()
        rule.onNodeWithText("Any account").performClick()

        assertEquals(null, picked)
    }

    @Test
    fun aRefusalBecauseTheFolderIsMissingShowsTheAlertUnderTheRefusedAccountWithAMoveAction() {
        val willow = nodes.first { it.name == "willow" }
        val atlas = nodes.first { it.name == "atlas" }
        val refused = row(atlas, "claude", "work@example.org", "64% left", false)

        rule.setContent {
            MyceliumTheme(true) {
                ModelPicker(
                    ModelPickerControls(
                        label = "Claude Opus 5.5",
                        options = options,
                        favourites = emptyList(),
                        providers = emptyList(),
                        onPickModel = {},
                        onFavourite = { _, _ -> },
                        accounts = listOf(refused, row(willow, "claude", "sam@example.com", "18% left", true)),
                        onPickAccount = { picked, done -> done(PickerRefusal("The folder /work/app is not on atlas.", picked.orEmpty(), "n2", "atlas")) },
                    ),
                )
            }
        }

        rule.onNodeWithContentDescription("Model: Claude Opus 5.5").performClick()
        rule.onNodeWithText("Account").performClick()
        rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").performClick()

        rule.onNodeWithText("Move to atlas").assertIsDisplayed()

        val row = rule.onNodeWithContentDescription("Claude Code · work@example.org on atlas, 64% left").getBoundsInRoot()
        val alert = rule.onNodeWithContentDescription("Error: The folder /work/app is not on atlas.").getBoundsInRoot()

        assertTrue(alert.top >= row.bottom, "the alert sits under the refused row")
    }

    @Test
    fun onTouchTheSheetKeepsTheAccessibleName() {
        rule.setContent {
            CompositionLocalProvider(LocalTouch provides true) {
                MyceliumTheme(true) {
                    ModelPicker(ModelPickerControls(label = "Claude Opus 5.5", options = options, favourites = emptyList(), providers = emptyList(), onPickModel = {}, onFavourite = { _, _ -> }))
                }
            }
        }

        rule.onNodeWithContentDescription("Model: Claude Opus 5.5").performClick()

        rule.onNodeWithContentDescription("Model").assertExists()
    }

    @Test
    fun theChevronNextToRetryNowOffersContinuingOnAnotherModel() {
        var picked: ModelOption? = null

        rule.setContent {
            MyceliumTheme(true) {
                UsageLimitCard(
                    UsageLimitNote("five_hour", "2026-10-23T15:30:00Z"),
                    "claude",
                    LimitActions(null, {}, {}, {}, ModelPickerControls(label = "Claude Opus 5.5", options = options, favourites = emptyList(), providers = emptyList(), onPickModel = { picked = it }, onFavourite = { _, _ -> })),
                )
            }
        }

        rule.onNodeWithContentDescription("Retry this conversation now").assertIsDisplayed()
        rule.onNodeWithContentDescription("Continue on another model").performClick()
        rule.onNodeWithContentDescription("Claude Code: Claude Sonnet 5.5").performClick()

        assertEquals("sonnet", picked?.model)
    }
}

