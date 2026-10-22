package mycelium

import mycelium.api.GitFile
import mycelium.state.ChangeRow
import mycelium.state.DiffLine
import mycelium.state.changeRows
import mycelium.state.commitProblem
import mycelium.state.diffIsStaged
import mycelium.state.firstChange
import mycelium.state.fullyStaged
import mycelium.state.parseUnifiedDiff
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

private fun file(path: String, staged: Boolean = false, unstaged: Boolean = true, untracked: Boolean = false) =
    GitFile(path, staged, unstaged, untracked, 0, 0)

class GitTest {
    @Test
    fun changesAreGroupedUnderTrackedAndUntrackedThenByFolderWithRootFilesFirst() {
        val rows = changeRows(
            listOf(
                file("src/b.kt"),
                file("README.md"),
                file("src/a.kt"),
                file("docs/guide.md"),
                file("notes/todo.txt", unstaged = false, untracked = true),
            ),
        )

        val described = rows.map {
            when (it) {
                is ChangeRow.Heading -> "# ${it.name}"
                is ChangeRow.Folder -> "/ ${it.path}"
                is ChangeRow.Change -> (if (it.indented) "  " else "") + it.name
            }
        }

        assertEquals(listOf("# Tracked", "README.md", "/ docs", "  guide.md", "/ src", "  a.kt", "  b.kt", "# Untracked", "/ notes", "  todo.txt"), described)
    }

    @Test
    fun aHeadingWithNoFilesIsLeftOut() {
        assertEquals(listOf(ChangeRow.Heading("Untracked")), changeRows(listOf(file("a", untracked = true))).take(1))
        assertEquals(emptyList(), changeRows(emptyList()))
    }

    @Test
    fun aFileIsStagedOnlyWhenNothingInItIsLeftUnstaged() {
        assertTrue(fullyStaged(file("a", staged = true, unstaged = false)))
        assertEquals(false, fullyStaged(file("a", staged = true, unstaged = true)))
        assertEquals(false, fullyStaged(file("a", staged = false, unstaged = false, untracked = true)))
    }

    @Test
    fun theDiffOfAPartlyStagedFileIsTheUnstagedPart() {
        assertEquals(false, diffIsStaged(file("a", staged = true, unstaged = true)))
        assertEquals(true, diffIsStaged(file("a", staged = true, unstaged = false)))
    }

    @Test
    fun theFirstChangeToViewPrefersTrackedFiles() {
        assertEquals("b", firstChange(listOf(file("a", unstaged = false, untracked = true), file("b")))?.path)
        assertEquals("a", firstChange(listOf(file("a", unstaged = false, untracked = true)))?.path)
        assertNull(firstChange(emptyList()))
    }

    @Test
    fun aCommitNeedsAMessageAndAStagedChange() {
        val staged = listOf(file("a", staged = true, unstaged = false))

        assertEquals("Enter a commit message.", commitProblem("  ", staged))
        assertEquals("Stage a change before you commit.", commitProblem("Fix", listOf(file("a"))))
        assertEquals("Stage a change before you commit.", commitProblem("Fix", emptyList()))
        assertNull(commitProblem("Fix", staged))
    }

    @Test
    fun aUnifiedDiffBecomesNumberedLinesAndSkipsItsFileHeader() {
        val diff = parseUnifiedDiff(
            "a.txt",
            """
            diff --git a/a.txt b/a.txt
            index 1..2 100644
            --- a/a.txt
            +++ b/a.txt
            @@ -3,3 +3,3 @@ fn main
             keep
            -old
            +new
            \ No newline at end of file
            """.trimIndent(),
        )

        assertEquals("a.txt", diff.path)
        assertEquals(
            listOf(
                DiffLine(DiffLine.Kind.Context, null, null, "@@ -3,3 +3,3 @@ fn main"),
                DiffLine(DiffLine.Kind.Context, 3, 3, "keep"),
                DiffLine(DiffLine.Kind.Removed, 4, null, "old"),
                DiffLine(DiffLine.Kind.Added, null, 4, "new"),
            ),
            diff.lines,
        )
        assertEquals(1, diff.added)
        assertEquals(1, diff.removed)
    }

    @Test
    fun aNewFileDiffNumbersFromOneAndABinaryDiffHasNoLines() {
        val added = parseUnifiedDiff("n.txt", "diff --git a/n.txt b/n.txt\nnew file mode 100644\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n")
        val binary = parseUnifiedDiff("i.png", "diff --git a/i.png b/i.png\nBinary files differ\n")

        assertEquals(listOf(1, 2), added.lines.mapNotNull { it.newNumber })
        assertEquals(emptyList(), binary.lines)
    }
}
