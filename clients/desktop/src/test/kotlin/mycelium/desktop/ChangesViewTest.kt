package mycelium.desktop

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.OutgoingContent
import io.ktor.http.headersOf
import kotlinx.coroutines.CompletableDeferred
import kotlinx.serialization.decodeFromString
import mycelium.api.Conversation
import mycelium.api.MyceliumApi
import mycelium.api.MyceliumJson
import mycelium.api.GitFileRequest
import mycelium.api.GitHunkRequest
import mycelium.api.RowsRequest
import mycelium.state.Store
import mycelium.state.changesTab
import mycelium.state.commitDiffTab
import mycelium.ui.ChangesTabView
import mycelium.ui.CommitTabView
import mycelium.ui.MyceliumTheme
import org.junit.Rule
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

private const val ConversationJson = """{"id":"c1","owner":"u","node_id":"n1","agent":"claude","cwd":"/work","title":"Importer","status":"review","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}"""

private const val ShowJson = """{"diff":"diff --git a/src/importer.kt b/src/importer.kt\n--- a/src/importer.kt\n+++ b/src/importer.kt\n@@ -1 +1 @@\n-old\n+new\n","truncated":false}"""

private class FileSpec(val path: String, val rows: Int, val staged: Boolean = false, val unstaged: Boolean = true, val untracked: Boolean = false) {
    val hunks = (0 until rows step 100).toList()

    fun layout(): String =
        """{"path":"$path","staged":$staged,"unstaged":$unstaged,"untracked":$untracked,"added":${rows / 2},"removed":${rows / 2},"rows":$rows,"hunks":$hunks,"fingerprint":"fp-$path"}"""

    fun row(index: Int): String = when {
        index in hunks -> """{"k":"h","t":"@@ -${index + 1},99 +${index + 1},99 @@"}"""
        index % 2 == 0 -> """{"k":"a","n":$index,"t":"val ${path.substringBefore('.')}Line$index = $index"}"""
        else -> """{"k":"d","o":$index,"t":"val ${path.substringBefore('.')}Old$index = $index"}"""
    }
}

private class ChangesFake(private val files: List<FileSpec>, private val rowsReady: CompletableDeferred<Unit>? = null) {
    val requests: MutableList<HttpRequestData> = CopyOnWriteArrayList()
    val rowRequests = AtomicInteger()

    val api = MyceliumApi(
        "https://mycelium.test",
        "token",
        MockEngine { request ->
            requests += request
            val path = request.url.encodedPath
            val body = when {
                path.endsWith("/git/changes-layout") -> """{"files":[${files.joinToString(",") { it.layout() }}]}"""
                path.endsWith("/git/changes-rows") -> {
                    rowRequests.incrementAndGet()
                    rowsReady?.await()
                    val asked = MyceliumJson.decodeFromString<RowsRequest>(bodyOf(request))

                    """{"ranges":[${asked.ranges.joinToString(",") { range ->
                        val spec = files.first { it.path == range.path }
                        val to = minOf(range.to, spec.rows)

                        """{"path":"${range.path}","from":${range.from},"fingerprint":"fp-${range.path}","rows":[${(range.from until to).joinToString(",") { spec.row(it) }}]}"""
                    }}]}"""
                }
                path.endsWith("/git/file") -> """{"content":"original text"}"""
                path.endsWith("/git/show") -> ShowJson
                else -> "{}"
            }

            respond(body, HttpStatusCode.OK, headersOf(HttpHeaders.ContentType, "application/json"))
        },
    )

    fun bodyOf(request: HttpRequestData): String = (request.body as OutgoingContent.ByteArrayContent).bytes().decodeToString()

    fun posts(route: String): List<HttpRequestData> = requests.filter { it.url.encodedPath.endsWith(route) }
}

@OptIn(ExperimentalTestApi::class)
class ChangesViewTest {
    @get:Rule
    val rule = createComposeRule()

    @get:Rule
    val scopes = TestScopes()

    private val composedRows = AtomicInteger()

    private fun open(fake: ChangesFake, reveal: String? = null, commit: Boolean = false) {
        val conversation = MyceliumJson.decodeFromString<Conversation>(ConversationJson)
        val store = Store(fake.api, scopes.create())

        rule.setContent {
            MyceliumTheme(true) {
                Box(Modifier.width(900.dp).height(600.dp)) {
                    if (commit) {
                        CommitTabView(store, conversation, commitDiffTab("abcdef1234567"), onRowComposed = { composedRows.incrementAndGet() })
                    } else {
                        ChangesTabView(store, conversation, changesTab(reveal), onRowComposed = { composedRows.incrementAndGet() })
                    }
                }
            }
        }
    }

    private fun await(description: String) {
        rule.waitUntil(10_000) { rule.onAllNodesWithContentDescription(description).fetchSemanticsNodes().isNotEmpty() }
    }

    private fun awaitText(text: String) {
        rule.waitUntil(10_000) { rule.onAllNodesWithText(text, substring = true).fetchSemanticsNodes().isNotEmpty() }
    }

    @Test
    fun aHunkActionIdentifiesTheDisplayedHunkAndItsOriginalDiskVersion() {
        val fake = ChangesFake(listOf(FileSpec("a.kt", 4)))
        open(fake)
        awaitText("Stage hunk")
        rule.onNodeWithText("Stage hunk").performClick()
        rule.waitUntil(10_000) { fake.posts("/git/hunk").isNotEmpty() }
        val request = MyceliumJson.decodeFromString<GitHunkRequest>(fake.bodyOf(fake.posts("/git/hunk").last()))
        assertEquals(GitHunkRequest("a.kt", 0, "fp-a.kt", "stage"), request)
    }

    @Test
    fun editingSavesTheNewTextWithTheOriginalContentToProtectExternalChanges() {
        val fake = ChangesFake(listOf(FileSpec("a.kt", 4)))
        open(fake)
        await("Edit file a.kt")
        rule.onNodeWithContentDescription("Edit file a.kt").performClick()
        await("Edit a.kt")
        rule.onNodeWithContentDescription("Edit a.kt").performTextReplacement("edited text")
        rule.onNodeWithText("Save").performClick()
        rule.waitUntil(10_000) { fake.posts("/git/file").size == 2 }
        val request = MyceliumJson.decodeFromString<GitFileRequest>(fake.bodyOf(fake.posts("/git/file").last()))
        assertEquals(GitFileRequest("a.kt", "edited text", "original text"), request)
    }

    @Test
    fun everyChangedFileIsStackedInOrderUnderItsOwnHeaderWithItsRows() {
        open(ChangesFake(listOf(FileSpec("a.kt", 4), FileSpec("b.kt", 4), FileSpec("notes/c.kt", 4))))

        await("Collapse notes/c.kt")
        awaitText("val aLine2")

        val tops = listOf("a.kt", "b.kt", "notes/c.kt").map { rule.onNodeWithContentDescription("Collapse $it").getUnclippedBoundsInRoot().top }

        assertEquals(tops.sorted(), tops)
        rule.onNodeWithText("val bLine2 = 2").assertIsDisplayed()
    }

    @Test
    fun aFileHeaderSaysItsStateInWordsAndOffersTheMatchingAction() {
        open(ChangesFake(listOf(FileSpec("a.kt", 4, staged = true, unstaged = false), FileSpec("b.kt", 4), FileSpec("new.kt", 2, unstaged = false, untracked = true))))

        await("Collapse new.kt")

        rule.onNodeWithText("Staged").assertIsDisplayed()
        rule.onNodeWithText("Unstaged").assertIsDisplayed()
        rule.onNodeWithText("Untracked").assertIsDisplayed()
        rule.onNodeWithContentDescription("Unstage file a.kt").assertIsDisplayed()
        rule.onNodeWithContentDescription("Stage file b.kt").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Stage file a.kt").fetchSemanticsNodes().size)
    }

    @Test
    fun loadingHunkControlsKeepsFollowingFileHeadersInPlace() {
        val rowsReady = CompletableDeferred<Unit>()
        val fake = ChangesFake(listOf(FileSpec("a.kt", 4), FileSpec("b.kt", 4)), rowsReady)
        open(fake)
        await("Stage file b.kt")
        val before = rule.onNodeWithContentDescription("Stage file b.kt").getUnclippedBoundsInRoot().top

        rowsReady.complete(Unit)
        awaitText("val bLine2 = 2")
        val after = rule.onNodeWithContentDescription("Stage file b.kt").getUnclippedBoundsInRoot().top

        assertEquals(before, after)
    }

    @Test
    fun stagingAFileFromItsHeaderPostsThatPathToTheStageRoute() {
        val rowsReady = CompletableDeferred<Unit>()
        val fake = ChangesFake(listOf(FileSpec("a.kt", 4), FileSpec("b.kt", 4)), rowsReady)
        open(fake)
        await("Stage file b.kt")
        rule.onNodeWithContentDescription("Stage file b.kt").assertIsDisplayed()

        rule.onNodeWithContentDescription("Stage file b.kt").performClick()

        rule.waitUntil(5_000) { fake.posts("/git/stage").isNotEmpty() }
        assertTrue(fake.bodyOf(fake.posts("/git/stage").single()).contains("\"b.kt\""))
        rowsReady.complete(Unit)
    }

    @Test
    fun collapsingAFileHidesItsRowsAndExpandingBringsThemBack() {
        open(ChangesFake(listOf(FileSpec("a.kt", 4), FileSpec("b.kt", 4))))
        awaitText("val aLine2")

        rule.onNodeWithContentDescription("Collapse a.kt").performClick()

        assertEquals(0, rule.onAllNodesWithText("val aLine2", substring = true).fetchSemanticsNodes().size)
        rule.onNodeWithText("val bLine2", substring = true).assertIsDisplayed()

        rule.onNodeWithContentDescription("Expand a.kt").performClick()

        awaitText("val aLine2")
    }

    @Test
    fun theKeyboardMovesBetweenFilesAndEnterCollapsesTheFocusedOne() {
        open(ChangesFake(listOf(FileSpec("a.kt", 4), FileSpec("b.kt", 4), FileSpec("c.kt", 4))))
        awaitText("val cLine2")

        rule.onRoot().performKeyInput { pressKey(Key.N) }
        rule.onRoot().performKeyInput { pressKey(Key.N) }
        rule.onRoot().performKeyInput { pressKey(Key.Enter) }

        await("Expand b.kt")
        assertEquals(0, rule.onAllNodesWithContentDescription("Expand a.kt").fetchSemanticsNodes().size)
    }

    @Test
    fun jAndKMoveBetweenHunksAcrossFilesSoEnterAfterJCollapsesTheFileOfThatHunk() {
        open(ChangesFake(listOf(FileSpec("a.kt", 4), FileSpec("b.kt", 4))))
        awaitText("val bLine2")

        repeat(3) { rule.onRoot().performKeyInput { pressKey(Key.J) } }
        rule.onRoot().performKeyInput { pressKey(Key.Enter) }

        await("Expand b.kt")
    }

    @Test
    fun selectingAFileInTheGitPanelSnapsToItWithItsHeaderAtTheTopAndItsRowsFetched() {
        val files = (0 until 40).map { FileSpec("f${it.toString().padStart(2, '0')}.kt", 30) }
        val fake = ChangesFake(files)
        open(fake, reveal = "f35.kt")

        await("Collapse f35.kt")
        awaitText("val f35Line2 = 2")

        val top = rule.onNodeWithContentDescription("Collapse f35.kt").getUnclippedBoundsInRoot().top

        assertTrue(top < 40.dp, "header at $top")
        assertEquals(0, rule.onAllNodesWithContentDescription("Collapse f00.kt").fetchSemanticsNodes().size)
    }

    @Test
    fun aLargeFileShowsItsFirstPartAndTheRemainingLinesOnRequest() {
        open(ChangesFake(listOf(FileSpec("big.kt", 2_500))))

        await("Collapse big.kt")
        rule.onNode(hasScrollAction()).performScrollToIndex(2_001)

        rule.onNodeWithText("Show the remaining 500 lines").performClick()

        rule.onNode(hasScrollAction()).performScrollToIndex(2_303)
        awaitText("val bigLine2302")
    }

    @Test
    fun aCommitOpensAsItsOwnStackOfFileSections() {
        open(ChangesFake(emptyList()), commit = true)

        await("Collapse src/importer.kt")

        rule.onNodeWithText("new").assertIsDisplayed()
        rule.onNodeWithText("old").assertIsDisplayed()
        assertEquals(0, rule.onAllNodesWithContentDescription("Stage file src/importer.kt").fetchSemanticsNodes().size)
    }

    @Test
    fun twoHundredFilesAndTwentyThousandLinesOpenWithOneLayoutCallOneRowRequestAndOnlyTheVisibleRowsComposed() {
        val files = (0 until 200).map { FileSpec("f${it.toString().padStart(3, '0')}.kt", 100) }
        val fake = ChangesFake(files)
        open(fake)

        awaitText("val f000Line2")

        val layoutCalls = fake.posts("/git/changes-layout").size
        val rowCalls = fake.rowRequests.get()
        val composedOpening = composedRows.get()

        rule.onNode(hasScrollAction()).performScrollToIndex(10_000)
        awaitText("val f0")
        rule.waitForIdle()


        assertEquals(1, layoutCalls)
        assertTrue(rowCalls <= 2, "row requests $rowCalls")
        assertTrue(composedOpening < 120, "composed $composedOpening")
        assertTrue(composedRows.get() < 300, "composed ${composedRows.get()}")
    }
}
