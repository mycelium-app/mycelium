package mycelium

import mycelium.api.ChangedFile
import mycelium.state.DiffLayout
import mycelium.state.DiffSection
import mycelium.state.changeSections
import mycelium.state.focusStep
import kotlin.test.Test
import kotlin.test.assertEquals

private fun section(path: String, rows: Int, hunks: List<Int> = listOf(0)) = DiffSection(path, null, 0, 0, rows, hunks)

class ChangesTest {
    @Test
    fun everySectionIsOneHeaderPlusItsRowsSoItemIndexesAreKnownBeforeAnyRowArrives() {
        val layout = DiffLayout(listOf(section("a", 10), section("b", 0), section("c", 5)), emptySet(), emptySet())

        assertEquals(listOf(0, 11, 13), layout.starts.toList())
        assertEquals(19, layout.size)
        assertEquals(13, layout.headerIndex("c"))
    }

    @Test
    fun aCollapsedSectionIsOnlyItsHeaderAndAFileOverTheLimitEndsWithAShowRemainingRow() {
        val layout = DiffLayout(listOf(section("a", 10), section("b", 30)), setOf("a"), emptySet(), limit = 20)

        assertEquals(listOf(0, 1), layout.starts.toList())
        assertEquals(1 + 1 + 20 + 1, layout.size)
        assertEquals(10, layout.hiddenRows[1])
    }

    @Test
    fun theRowWindowAroundTheViewportIsClippedToEachFilesRows() {
        val layout = DiffLayout(listOf(section("a", 10), section("b", 100)), emptySet(), emptySet())
        val windows = layout.windows(first = 8, last = 14, margin = 0)

        assertEquals(listOf("a" to (7..10), "b" to (0..3)), windows.map { it.path to (it.from..it.to) })
    }

    @Test
    fun jAndKStepThroughHunksAcrossFilesAndNAndPThroughFiles() {
        val layout = DiffLayout(listOf(section("a", 300, listOf(0, 100)), section("b", 4)), emptySet(), emptySet())

        assertEquals("h:a", focusStep(layout, null, 1, byFile = true))
        assertEquals("k:0:a", focusStep(layout, null, 1, byFile = false))
        assertEquals("k:100:a", focusStep(layout, "k:0:a", 1, byFile = false))
        assertEquals("k:0:b", focusStep(layout, "k:100:a", 1, byFile = false))
        assertEquals("h:b", focusStep(layout, "k:100:a", 1, byFile = true))
        assertEquals("k:100:a", focusStep(layout, "k:0:b", -1, byFile = false))
        assertEquals("k:0:b", focusStep(layout, "k:0:b", 1, byFile = false))
    }

    @Test
    fun filesAreOrderedLikeTheGitPanelWithUntrackedLastAndTaggedInWords() {
        val files = listOf(
            ChangedFile("z.kt", staged = false, unstaged = false, untracked = true, added = 1, removed = 0, rows = 2, fingerprint = "z"),
            ChangedFile("src/b.kt", staged = true, unstaged = true, untracked = false, added = 1, removed = 1, rows = 3, fingerprint = "b"),
            ChangedFile("a.kt", staged = true, unstaged = false, untracked = false, added = 1, removed = 1, rows = 3, fingerprint = "a"),
        )

        val sections = changeSections(files)

        assertEquals(listOf("a.kt", "src/b.kt", "z.kt"), sections.map { it.path })
        assertEquals(listOf("Staged", "Staged and unstaged", "Untracked"), sections.map { it.tag })
        assertEquals(listOf(false, true, true), sections.map { it.canStage })
    }
}
