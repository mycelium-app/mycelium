package mycelium.desktop

import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasText
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import mycelium.ui.Accent
import mycelium.ui.Appearance
import mycelium.ui.Display
import mycelium.ui.LocalAppearance
import mycelium.ui.TextSize
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.isSelected
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.graphics.asSkiaBitmap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlin.time.Instant
import kotlinx.coroutines.runBlocking
import mycelium.api.Activity
import mycelium.api.ActivityRow
import mycelium.api.Me
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.state.Store
import mycelium.ui.MyceliumTheme
import mycelium.ui.Route
import mycelium.ui.Shell

import mycelium.ui.tokenRate
import mycelium.ui.tokenSample
import org.junit.Rule
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private fun conversation(status: String, actionItems: List<Pair<String, String>> = emptyList()): String {
    val items = actionItems.joinToString(",") { (id, text) -> """{"id":"$id","text":"$text","created_at":"2026-10-22T10:0${id.last()}:00Z"}""" }

    val itemsField = if (items.isEmpty()) "" else ",\"action_items\":[$items]"

    return """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work/harbour","title":"Bump the UI kit","status":"$status",
"acp_session_id":null,"mode":null,"model":null,"created_at":"2026-10-22T10:00:00Z","updated_at":"2026-10-22T10:05:00Z","archived_at":null$itemsField}"""
}

private const val PermissionEvents = """[{"conversation_id":"c1","seq":1,"kind":"permission_request","payload":{"request_id":"r1","toolCall":{"title":"pnpm db:migrate"},
"options":[{"optionId":"approve","name":"Approve","kind":"allow_once"},{"optionId":"deny","name":"Deny","kind":"reject_once"}]}}]"""

private const val StartProposalJson = """{"id":"p1","conversation_id":"c1","kind":"subagent","title":"Review the schema","state":"proposed","created_at":"2026-10-22T10:06:00Z",
"steps":[{"key":"review","title":"Review the schema","node":{"id":"n1","name":"atlas","online":true},"folder":"/work/harbour","agent":"claude","account_choices":["work","personal"],"first_message":"Read the migrations and list anything that breaks old clients."}]}"""

private const val NoFeed = """{"posts":[]}"""

private const val VoiceOff = """{"voice_enabled":false}"""

private const val VoiceOn = """{"voice_enabled":true,"speech_to_text_provider":null,"text_to_speech_provider":null}"""

private fun feedPost(id: String, message: String, project: String? = "harbour", createdAt: String = "2026-10-22T10:00:00Z") = """{"id":"$id","author":"Scribe","message":"$message","project":${project?.let { "\"$it\"" }},"conversation_id":"c1","conversation_title":"Restore flow copy","created_at":"$createdAt"}"""

class InboxTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val posts = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()

    private val platform = AppsPlatform()

    private val feedRequests = java.util.concurrent.CopyOnWriteArrayList<String>()

    private val deletes = java.util.concurrent.CopyOnWriteArrayList<String>()

    private fun show(status: String, feed: String = NoFeed, privacy: String = VoiceOff, textSize: TextSize = TextSize.Default, density: Float = 1f, events: String? = null, actionItems: List<Pair<String, String>> = emptyList(), startProposals: String = "[]"): Store {
        val open = actionItems.toMutableList()
        val engine = MockEngine { request ->
            val path = request.url.encodedPath

            if (request.method == HttpMethod.Post) {
                posts += path to String(request.body.toByteArray())
            }

            if (request.method == HttpMethod.Delete) {
                deletes += path
                open.removeAll { path.endsWith("/action-items/${it.first}") }
            }

            val body = when {
                path.endsWith("/inbox/feed") -> feed
                path.endsWith("/privacy") -> privacy
                path.endsWith("/start-proposals") -> startProposals
                request.method == HttpMethod.Delete -> conversation(status, open)
                path.endsWith("/conversations") -> "[${conversation(status, open)}]"
                path.endsWith("/conversations/c1/events") -> events ?: if (status == "input") PermissionEvents else "[]"
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/workflows") || path.endsWith("/queue") || path.endsWith("/session-updates") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking { store.refresh() }
        rule.setContent { CompositionLocalProvider(LocalDensity provides Density(density), LocalAppearance provides Appearance(true, Accent.Neutral, emptyList(), {}, display = Display(textSize = textSize))) { MyceliumTheme(true) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Inbox) } } }

        return store
    }

    @Test
    fun aPermissionFromAConversationAwaitingInputIsApprovedFromTheInbox() {
        show("input")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve pnpm db:migrate").fetchSemanticsNodes().isNotEmpty() }

        assertTrue(rule.onAllNodesWithText("Bump the UI kit").fetchSemanticsNodes().isNotEmpty())
        rule.onNodeWithText("Approve").performClick()

        rule.waitUntil(5_000) { posts.any { it.first.endsWith("/conversations/c1/permission") } }
        assertTrue(posts.single { it.first.endsWith("/conversations/c1/permission") }.second.contains("\"approve\""))
    }

    @Test
    fun anInboxItemWithoutAProjectOrAssistantStartsItsNameAtTheRowContentStart() {
        show("input")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve pnpm db:migrate").fetchSemanticsNodes().isNotEmpty() }

        val left = rule.onNodeWithText("harbour", useUnmergedTree = true).getUnclippedBoundsInRoot().left

        assertEquals(rule.onNodeWithText("Needs you", useUnmergedTree = true).getUnclippedBoundsInRoot().left, left)
    }

    @Test
    fun theInboxRendersForComparisonWithTheArtboard() {
        show("input")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve pnpm db:migrate").fetchSemanticsNodes().isNotEmpty() }
        val png = org.jetbrains.skia.Image.makeFromBitmap(rule.onRoot().captureToImage().asSkiaBitmap()).encodeToData(org.jetbrains.skia.EncodedImageFormat.PNG)!!.bytes
        val out = java.io.File("/tmp/mycelium-build-chats-row/shots/inbox.png")
        out.parentFile.mkdirs()
        out.writeBytes(png)

        assertEquals(true, out.length() > 5_000)
    }

    @Test
    fun aStartProposalOfAWorkingConversationIsInNeedsYouWithItsCreateButton() {
        show("working", startProposals = "[$StartProposalJson]")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Proposed a subagent: Review the schema").fetchSemanticsNodes().isNotEmpty() }

        assertTrue(rule.onAllNodesWithText("Bump the UI kit").fetchSemanticsNodes().isNotEmpty())
        rule.onNodeWithText("Waiting for approval").assertExists()
        rule.onNodeWithText("Create").assertExists()
        rule.onNodeWithText("Don't create").assertExists()
    }

    @Test
    fun theInboxWithAStartProposalOfAWorkingConversationRenders() {
        val engine = MockEngine { request ->
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/start-proposals") -> "[$StartProposalJson]"
                path.endsWith("/conversations") -> "[${conversation("working")}]"
                path.endsWith("/inbox/feed") -> NoFeed
                path.endsWith("/privacy") -> VoiceOff
                path.endsWith("/nodes") || path.endsWith("/groups") || path.endsWith("/workflows") || path.endsWith("/queue") || path.endsWith("/session-updates") || path.endsWith("/events") -> "[]"
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        }
        val store = Store(MyceliumApi("https://mycelium.test", "token", engine), scopes.create())

        runBlocking {
            store.refresh()
            store.refreshStartProposals()
        }

        val png = renderAt("inbox-start-proposal", true, false, height = 1530) { Shell(store, platform, Me("u", "sam@example.com", "Sam"), onSignOut = {}, initialRoute = Route.Inbox) }

        assertTrue(png.length() > 10_000)
    }

    @Test
    fun anApprovedItemStaysResolvedBrieflyThenLeaves() {
        val store = show("input")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve pnpm db:migrate").fetchSemanticsNodes().isNotEmpty() }

        store.receive(mycelium.api.StreamMessage.EventAppended(mycelium.api.Event("c1", 2, "permission_resolved", MyceliumJson.parseToJsonElement("""{"request_id":"r1"}"""))))
        store.receive(mycelium.api.StreamMessage.ConversationChanged(MyceliumJson.decodeFromString(mycelium.api.Conversation.serializer(), conversation("working"))))
        rule.waitForIdle()

        rule.onNodeWithText("Answered").assertExists()

        rule.mainClock.advanceTimeBy(5_000)
        rule.waitForIdle()

        rule.onNodeWithText("Nothing needs you").assertExists()
    }

    @Test
    fun actionItemsShowAsSeparateItemsEvenWhenTheConversationIsNotWaiting() {
        show("done", actionItems = listOf("i1" to "Update Mycelium on atlas", "i2" to "Check the harbour backups"))
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Update Mycelium on atlas").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(2, rule.onAllNodesWithText("For you to do").fetchSemanticsNodes().size)
        rule.onNodeWithText("Check the harbour backups").assertExists()
    }

    @Test
    fun markingAnActionItemDoneDeletesItThenItLeaves() {
        show("done", actionItems = listOf("i1" to "Update Mycelium on atlas"))
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Mark as done: Update Mycelium on atlas").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Mark as done: Update Mycelium on atlas").performClick()
        rule.waitUntil(5_000) { deletes.isNotEmpty() }
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Done").fetchSemanticsNodes().isNotEmpty() }

        assertEquals(listOf("/api/v1/conversations/c1/action-items/i1"), deletes.toList())
        rule.onNodeWithText("Marked as done. It leaves Needs you.").assertExists()

        rule.mainClock.advanceTimeBy(5_000)
        rule.waitForIdle()

        rule.onNodeWithText("Nothing needs you").assertExists()
    }

    @Test
    fun anActionItemStaysWhenTheConversationChangesStatus() {
        val store = show("input", actionItems = listOf("i1" to "Update Mycelium on atlas"))
        rule.waitUntil(5_000) { rule.onAllNodesWithText("Update Mycelium on atlas").fetchSemanticsNodes().isNotEmpty() }

        store.receive(mycelium.api.StreamMessage.ConversationChanged(MyceliumJson.decodeFromString(mycelium.api.Conversation.serializer(), conversation("working", listOf("i1" to "Update Mycelium on atlas")))))
        rule.mainClock.advanceTimeBy(5_000)
        rule.waitForIdle()

        rule.onNodeWithText("For you to do").assertExists()
        rule.onNodeWithText("Update Mycelium on atlas").assertExists()
    }

    @Test
    fun aPostShowsItsAuthorAndALinkToItsChat() {
        show("done", """{"posts":[${feedPost("p1", "Search now finds conversations by branch name.")}]}""")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Search now finds conversations by branch name.").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("Scribe").assertExists()
        rule.onNodeWithContentDescription("Open chat: harbour, Restore flow copy").assertExists()
    }

    @Test
    fun feedBodiesRenderFormattingListsCodeAndLinks() {
        val message = "**Released** the `plan`\\n\\n- [x] Publish notes\\n\\n```text\\nsample command\\n```\\n\\n[the docs](https://example.com/docs)"
        show("done", """{"posts":[${feedPost("p1", message)}]}""")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Released the plan").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Done").assertExists()
        rule.onNodeWithContentDescription("Copy text code").assertExists()
        rule.onNodeWithText("the docs").performClick()
        assertEquals(listOf("https://example.com/docs"), platform.opened)
        rule.onNodeWithContentDescription("Open chat: harbour, Restore flow copy").assertExists()
    }

    @Test
    fun needsYouFullBodyRendersListsAndCodeWithReplyControlsIntact() {
        val events = """[{"conversation_id":"c1","seq":1,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"**Review** the plan\n\n- [x] Check notes\n\n```text\nsample command\n```"}}}}]"""
        show("input", events = events)

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Review the plan").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Done").assertExists()
        rule.onNodeWithContentDescription("Copy text code").assertExists()
        rule.onNodeWithText("Send reply").assertExists()
    }

    @Test
    fun expandedNeedsYouPreviewsRenderInlineFormattingAndLinks() {
        val events = """[{"conversation_id":"c1","seq":1,"kind":"user_message","payload":{"text":"[the **docs**](https://example.com/docs)"}},{"conversation_id":"c1","seq":2,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"**Check** the `plan`"}}}},{"conversation_id":"c1","seq":3,"kind":"permission_request","payload":{"request_id":"r1","toolCall":{"title":"pnpm db:migrate"},"options":[{"optionId":"approve","name":"Approve","kind":"allow_once"}]}}]"""
        show("input", events = events)

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Show more of Bump the UI kit").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Show more of Bump the UI kit").performClick()
        rule.onNodeWithText("You: the docs").performClick()
        assertEquals(listOf("https://example.com/docs"), platform.opened)
        assertEquals(2, rule.onAllNodesWithText("Check the plan").fetchSemanticsNodes().size)
        rule.onNodeWithText("**Check** the `plan`").assertDoesNotExist()
    }

    @Test
    fun contextAndExpandedPreviewsKeepTheirTwoAndFourLineLimits() {
        val text = "**Review** " + "the release notes ".repeat(100)
        val encoded = kotlinx.serialization.json.Json.encodeToString(kotlinx.serialization.serializer<String>(), text)
        val events = """[{"conversation_id":"c1","seq":1,"kind":"acp_update","payload":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":$encoded}}}},{"conversation_id":"c1","seq":2,"kind":"permission_request","payload":{"request_id":"r1","toolCall":{"title":"pnpm db:migrate"},"options":[{"optionId":"approve","name":"Approve","kind":"allow_once"}]}}]"""
        show("input", events = events)

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Show more of Bump the UI kit").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithContentDescription("Show more of Bump the UI kit").performClick()
        val layouts = rule.onAllNodesWithText("Review " + "the release notes ".repeat(100)).fetchSemanticsNodes().map { node ->
            val results = mutableListOf<androidx.compose.ui.text.TextLayoutResult>()
            node.config[androidx.compose.ui.semantics.SemanticsActions.GetTextLayoutResult].action?.invoke(results)
            results.single()
        }

        assertEquals(listOf(2, 4), layouts.map { it.lineCount }.sorted())
        assertTrue(layouts.all { it.multiParagraph.didExceedMaxLines })
        assertTrue(layouts.all { it.layoutInput.overflow == androidx.compose.ui.text.style.TextOverflow.Ellipsis })
    }

    @Test
    fun aPostWithoutAProjectLinksByChatTitleAlone() {
        show("done", """{"posts":[${feedPost("p1", "The billing guide is published.", project = null)}]}""")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open chat: Restore flow copy").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun thePostLinkOpensItsChat() {
        show("done", """{"posts":[${feedPost("p1", "The restore flow copy is live.")}]}""")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open chat: harbour, Restore flow copy").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Open chat: harbour, Restore flow copy").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithText("The restore flow copy is live.").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun aPostDrawsItsInlineBadgesWithAccessibleNames() {
        show("done", """{"posts":[${feedPost("p1", "[[project:Mycelium]] [[type:release]] v1 is [[outcome:live]] with [[custom:Nightly|moon]] and [[nope:x]].", project = "Mycelium")}]}""")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Outcome: live").fetchSemanticsNodes().isNotEmpty() }
        for (name in listOf("Project: Mycelium", "Type: release", "Outcome: live", "Nightly")) {
            rule.onNode(hasContentDescription(name) and SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Image)).assertExists()
        }

        rule.onNodeWithText("Mycelium release v1 is live with Nightly and [[nope:x]].").assertExists()
        rule.onNodeWithContentDescription("Open chat: Mycelium, Restore flow copy").assertHasClickAction()
    }

    private fun assertEveryInlineBadgeShowsItsLabelInFull(size: TextSize, density: Float) {
        val message = "[[project:Mycelium]] [[type:release]] [[outcome:live]] [[project:Platform]] shipped."
        show("done", """{"posts":[${feedPost("p1", message)}]}""", textSize = size, density = density)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Outcome: live").fetchSemanticsNodes().isNotEmpty() }

        for ((name, label) in listOf("Project: Mycelium" to "Mycelium", "Type: release" to "release", "Outcome: live" to "live", "Project: Platform" to "Platform")) {
            val node = rule.onNode(hasText(label) and hasAnyAncestor(hasContentDescription(name)), useUnmergedTree = true)
            val results = mutableListOf<TextLayoutResult>()
            node.fetchSemanticsNode().config[SemanticsActions.GetTextLayoutResult].action?.invoke(results)

            assertTrue(results.isNotEmpty() && !results.single().hasVisualOverflow && results.single().lineCount == 1, "$label is cut off at ${size.label} text size and density $density")
        }
    }

    @Test
    fun everyInlineBadgeShowsItsLabelInFullAtTheDefaultTextSize() {
        assertEveryInlineBadgeShowsItsLabelInFull(TextSize.Default, 1f)
    }

    @Test
    fun everyInlineBadgeShowsItsLabelInFullAtTheLargestTextSize() {
        assertEveryInlineBadgeShowsItsLabelInFull(TextSize.ExtraLarge, 1f)
    }

    @Test
    fun everyInlineBadgeShowsItsLabelInFullOnAFractionalDensityAtTheDefaultTextSize() {
        assertEveryInlineBadgeShowsItsLabelInFull(TextSize.Default, 0.9f)
    }

    @Test
    fun aProjectBadgeReplacesTheProjectNameOnThePostRow() {
        show("done", """{"posts":[${feedPost("p1", "[[project:harbour]] The copy is out.")}, ${feedPost("p2", "The guide is out.", project = "atlas")}]}""")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Project: harbour").fetchSemanticsNodes().isNotEmpty() }
        rule.onNodeWithText("harbour").assertDoesNotExist()
        rule.onNodeWithText("atlas").assertExists()
    }

    @Test
    fun theChatTitleIsNotShownOnAPostButItsChatStillOpens() {
        show("done", """{"posts":[${feedPost("p1", "[[type:release]] The copy is out.")}]}""")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open chat: harbour, Restore flow copy").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Restore flow copy").assertDoesNotExist()
        rule.onNodeWithContentDescription("Open chat: harbour, Restore flow copy").performClick()

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Type: release").fetchSemanticsNodes().isEmpty() }
    }

    @Test
    fun anEmptyFeedSaysNoPostsYet() {
        show("done")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("No posts yet").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aPostArrivingOnTheStreamAppearsAboveEarlierPosts() {
        val store = show("done", """{"posts":[${feedPost("p1", "The first release is out.")}]}""")
        rule.waitUntil(5_000) { rule.onAllNodesWithText("The first release is out.").fetchSemanticsNodes().isNotEmpty() }

        store.receive(mycelium.api.StreamMessage.FeedPosted(MyceliumJson.decodeFromString(mycelium.api.InboxPost.serializer(), feedPost("p2", "The second release is out.", createdAt = "2026-10-22T11:00:00Z"))))

        rule.waitUntil(5_000) { rule.onAllNodesWithText("The second release is out.").fetchSemanticsNodes().isNotEmpty() }
        val newest = rule.onNodeWithText("The second release is out.").getUnclippedBoundsInRoot().top
        val earlier = rule.onNodeWithText("The first release is out.").getUnclippedBoundsInRoot().top
        assertTrue(newest < earlier)
    }

    @Test
    fun aNeedsYouFocusMessageOutlinesTheMatchingItem() {
        val store = show("input")
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve pnpm db:migrate").fetchSemanticsNodes().isNotEmpty() }
        val before = rule.onAllNodes(isSelected()).fetchSemanticsNodes().size

        store.receive(mycelium.api.StreamMessage.NeedsYouFocused("c1/e1"))
        rule.waitForIdle()

        assertEquals(before + 1, rule.onAllNodes(isSelected()).fetchSemanticsNodes().size)
    }

    @Test
    fun withVoiceOpenTheTopItemIsOutlinedAndTheVoicePanelDocksInTheColumn() {
        val store = show("input", privacy = VoiceOn)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve pnpm db:migrate").fetchSemanticsNodes().isNotEmpty() }
        rule.waitUntil(5_000) { store.voice.state.value.privacy.voiceEnabled }
        val before = rule.onAllNodes(isSelected()).fetchSemanticsNodes().size

        rule.runOnIdle { store.voice.startListening() }
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open the Voice thread").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Talk or type to Voice").assertExists()
        assertEquals(before + 1, rule.onAllNodes(isSelected()).fetchSemanticsNodes().size)
    }

    @Test
    fun withVoiceEnabledTheDockShowsBelowNeedsYouWithNothingOpened() {
        val store = show("input", privacy = VoiceOn)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Open the Voice thread").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithText("Talk or type to Voice").assertExists()
        rule.onNodeWithContentDescription("Start listening (Ctrl+Shift+Space)").assertExists()
        assertTrue(!store.voice.state.value.open)
    }

    @Test
    fun withVoiceDisabledTheDockStillTakesTypingWithoutAMicrophone() {
        show("input", privacy = VoiceOff)
        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("Approve pnpm db:migrate").fetchSemanticsNodes().isNotEmpty() }

        rule.onNodeWithContentDescription("Open the Voice thread").assertExists()
        rule.onNodeWithText("Talk or type to Voice").assertExists()
        assertTrue(rule.onAllNodesWithContentDescription("Start listening (Ctrl+Shift+Space)").fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun nothingPendingSaysSo() {
        show("done")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Nothing needs you").fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun theSidebarLinkCountsWhatNeedsYou() {
        show("input")

        rule.waitUntil(5_000) { rule.onAllNodesWithContentDescription("1 waiting").fetchSemanticsNodes().isNotEmpty() }
        assertTrue(rule.onAllNodesWithText("Inbox").fetchSemanticsNodes().isNotEmpty())
    }

    @Test
    fun theSidebarLinkShowsNoCountWhenNothingNeedsYou() {
        show("done")

        rule.waitUntil(5_000) { rule.onAllNodesWithText("Nothing needs you").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, rule.onAllNodesWithContentDescription("0 waiting").fetchSemanticsNodes().size)
    }

    @Test
    fun tokensPerSecondIsTheTokenDeltaOverElapsedTime() {
        fun activity(tokens: Long, running: Long) = Activity("today", ActivityRow("total", "Total", inputTokens = tokens), conversations = listOf(ActivityRow("c1", "Build", inputTokens = running)))

        val first = tokenSample(activity(1_000, 100), Instant.parse("2026-10-22T10:00:00Z"))
        val second = tokenSample(activity(1_600, 400), Instant.parse("2026-10-22T10:00:15Z"))
        val rate = tokenRate(first, second, activity(1_600, 400), earlierPeak = 0)

        assertEquals(40L, rate.perSecond)
        assertEquals(20L, rate.running.getValue("c1"))
        assertEquals(40L, rate.peak)
    }
}
