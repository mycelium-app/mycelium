package mycelium.state

import mycelium.api.Artifact
import mycelium.api.ArtifactKinds

const val AllArtifacts = "All"
const val YourArtifacts = "Yours"

val ArtifactTabs = listOf(AllArtifacts, YourArtifacts)

val ArtifactKindFilters = listOf(
    ArtifactKinds.Design to "Design",
    ArtifactKinds.Page to "Page",
    ArtifactKinds.Markdown to "Markdown",
    ArtifactKinds.File to "File",
)

fun artifactKindName(kind: String): String = ArtifactKindFilters.firstOrNull { it.first == kind }?.second ?: kind.replaceFirstChar { it.uppercase() }

fun artifactOpenUrl(baseUrl: String, id: String, kind: String, version: Int): String {
    val root = baseUrl.trimEnd('/')

    return if (kind == ArtifactKinds.Design) "$root/design/$id" else "$root/api/v1/artifacts/$id/versions/$version/content"
}

fun artifactOpenUrl(baseUrl: String, artifact: Artifact): String = artifactOpenUrl(baseUrl, artifact.id, artifact.kind, artifact.latestVersion)

fun visibleArtifacts(artifacts: Collection<Artifact>, tab: String, ownerId: String, kind: String?, query: String): List<Artifact> {
    val needle = query.trim()

    return artifacts
        .filter { tab != YourArtifacts || it.owner == ownerId }
        .filter { kind == null || it.kind == kind }
        .filter { needle.isEmpty() || it.title.contains(needle, ignoreCase = true) }
        .sortedByDescending { it.updatedAt }
}
