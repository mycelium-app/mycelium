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
    ArtifactKinds.Drawing to "Drawing",
    ArtifactKinds.Typst to "Typst",
    ArtifactKinds.Doc to "Doc",
    ArtifactKinds.Slides to "Slides",
    ArtifactKinds.Dashboard to "Dashboard",
    ArtifactKinds.Motion to "Motion",
    ArtifactKinds.DesignSystem to "Design system",
)

val NewArtifactKinds = listOf(
    Triple(ArtifactKinds.Doc, "Doc", "Rich text with headings and lists"),
    Triple(ArtifactKinds.Slides, "Slides", "A deck to present"),
    Triple(ArtifactKinds.Drawing, "Drawing", "An Excalidraw canvas"),
    Triple(ArtifactKinds.Typst, "Typst", "Typeset, downloads as PDF"),
    Triple(ArtifactKinds.Dashboard, "Dashboard", "Grafana-compatible panels"),
    Triple(ArtifactKinds.Motion, "Motion", "An animated scene"),
    Triple(ArtifactKinds.Design, "Design", "Artboards for a screen"),
    Triple(ArtifactKinds.DesignSystem, "Design system", "Tokens, type and components"),
    Triple(ArtifactKinds.Markdown, "Markdown", "Plain text with formatting"),
)

private val EditorKinds = setOf(ArtifactKinds.Design, ArtifactKinds.Drawing, ArtifactKinds.Typst, ArtifactKinds.Doc, ArtifactKinds.Slides, ArtifactKinds.Dashboard, ArtifactKinds.Motion, ArtifactKinds.DesignSystem)

fun newArtifactNameProblem(kind: String, name: String): String? =
    if (name.isBlank()) "Enter a name for the ${artifactKindName(kind).lowercase()}" else null

fun designSystemsOf(artifacts: Collection<Artifact>): List<Artifact> =
    artifacts.filter { it.kind == ArtifactKinds.DesignSystem }.sortedBy { it.title.lowercase() }

fun designsUsing(artifacts: Collection<Artifact>, designSystemId: String): List<Artifact> =
    artifacts.filter { it.kind == ArtifactKinds.Design && it.designSystemId == designSystemId }.sortedBy { it.title.lowercase() }

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

    return if (kind in EditorKinds) "$root/design/$id?theme=$theme&accent=$accent" else "$root/api/v1/artifacts/$id/versions/$version/content"
}

fun artifactOpenUrl(baseUrl: String, artifact: Artifact, dark: Boolean = true, accent: String = "neutral"): String =
    artifactOpenUrl(baseUrl, artifact.id, artifact.kind, artifact.latestVersion, dark, accent)

fun visibleArtifacts(artifacts: Collection<Artifact>, tab: String, ownerId: String, kind: String?, query: String, projectId: String? = null): List<Artifact> {
    val needle = query.trim()

    return artifacts
        .filter { tab != YourArtifacts || it.owner == ownerId }
        .filter { kind == null || it.kind == kind }
        .filter { projectId == null || it.origin?.projectId == projectId }
        .filter { needle.isEmpty() || it.title.contains(needle, ignoreCase = true) }
        .sortedByDescending { it.updatedAt }
}

fun artifactProjects(artifacts: Collection<Artifact>): List<Choice> =
    artifacts
        .mapNotNull { artifact -> artifact.origin?.let { origin -> origin.projectId?.let { id -> Choice(id, origin.projectName.orEmpty()) } } }
        .distinctBy { it.id }
        .sortedBy { it.name.lowercase() }
