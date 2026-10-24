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

enum class ArtifactPreview { Snapshot, Text, Picture, Icon }

const val PreviewLines = 12

fun artifactPreview(artifact: Artifact): ArtifactPreview = when {
    artifact.kind == ArtifactKinds.Markdown -> ArtifactPreview.Text
    artifact.kind == ArtifactKinds.File && artifact.mediaType?.startsWith("image/") == true -> ArtifactPreview.Picture
    artifact.thumbnailVersion != null -> ArtifactPreview.Snapshot
    else -> ArtifactPreview.Icon
}

fun artifactKindName(kind: String): String = ArtifactKindFilters.firstOrNull { it.first == kind }?.second ?: kind.replaceFirstChar { it.uppercase() }

fun artifactOpenUrl(baseUrl: String, id: String, kind: String, version: Int, dark: Boolean = true, accent: String = "neutral"): String {
    val root = baseUrl.trimEnd('/')
    val theme = if (dark) "dark" else "light"

    return if (kind == ArtifactKinds.Design) "$root/design/$id?theme=$theme&accent=$accent" else "$root/api/v1/artifacts/$id/versions/$version/content"
}

fun artifactOpenUrl(baseUrl: String, artifact: Artifact, dark: Boolean = true, accent: String = "neutral"): String =
    artifactOpenUrl(baseUrl, artifact.id, artifact.kind, artifact.latestVersion, dark, accent)

fun visibleArtifacts(artifacts: Collection<Artifact>, tab: String, ownerId: String, kind: String?, query: String): List<Artifact> {
    val needle = query.trim()

    return artifacts
        .filter { tab != YourArtifacts || it.owner == ownerId }
        .filter { kind == null || it.kind == kind }
        .filter { needle.isEmpty() || it.title.contains(needle, ignoreCase = true) }
        .sortedByDescending { it.updatedAt }
}
