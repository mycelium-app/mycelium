package mycelium

import mycelium.state.parseChangelog
import mycelium.state.releasesSince
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

private val sample = """
# Changelog

## Unreleased

- Next thing.

## v0.4.0

- Artifacts.
- History table.

## v0.3.13

- Close session.

## v0.3.9

- Older fix.
""".trimIndent()

class ChangelogTest {
    @Test
    fun `a changelog parses into releases newest first with their bullets`() {
        val releases = parseChangelog(sample)

        assertEquals(listOf("Unreleased", "v0.4.0", "v0.3.13", "v0.3.9"), releases.map { it.title })
        assertEquals(listOf("Artifacts.", "History table."), releases[1].notes)
    }

    @Test
    fun `an unreleased section has no version and a tagged one drops the v`() {
        val releases = parseChangelog(sample)

        assertNull(releases[0].version)
        assertEquals("0.4.0", releases[1].version)
    }

    @Test
    fun `every release newer than the last launched version up to the running one is listed`() {
        val releases = releasesSince(parseChangelog(sample), "0.3.9", "0.4.0")

        assertEquals(listOf("v0.4.0", "v0.3.13"), releases.map { it.title })
    }

    @Test
    fun `versions compare by number so 0 3 10 is newer than 0 3 9`() {
        val text = "## v0.3.10\n\n- Ten.\n\n## v0.3.9\n\n- Nine.\n"

        assertEquals(listOf("v0.3.10"), releasesSince(parseChangelog(text), "0.3.9", "0.3.10").map { it.title })
    }

    @Test
    fun `nothing is listed on a first launch, when already current, or for a development build`() {
        val releases = parseChangelog(sample)

        assertEquals(emptyList(), releasesSince(releases, null, "0.4.0"))
        assertEquals(emptyList(), releasesSince(releases, "0.4.0", "0.4.0"))
        assertEquals(emptyList(), releasesSince(releases, "0.3.9", "dev"))
    }

    @Test
    fun `releases newer than the running version are left out`() {
        val releases = releasesSince(parseChangelog(sample), "0.3.9", "0.3.13")

        assertEquals(listOf("v0.3.13"), releases.map { it.title })
    }
}
