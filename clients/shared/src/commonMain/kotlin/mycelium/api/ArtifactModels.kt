package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class ArtifactOrigin(
    @SerialName("conversation_id") val conversationId: String,
    @SerialName("conversation_title") val conversationTitle: String,
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("project_name") val projectName: String? = null,
    @SerialName("project_icon") val projectIcon: String? = null,
)

@Serializable
data class Artifact(
    val id: String,
    val owner: String,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("project_ids") val projectIds: List<String>? = null,
    @SerialName("conversation_ids") val conversationIds: List<String>? = null,
    val origin: ArtifactOrigin? = null,
    val kind: String,
    @SerialName("design_system_id") val designSystemId: String? = null,
    val title: String,
    val slug: String = "",
    @SerialName("public") val isPublic: Boolean = false,
    @SerialName("latest_version") val latestVersion: Int = 1,
    @SerialName("thumbnail_version") val thumbnailVersion: Int? = null,
    @SerialName("media_type") val mediaType: String? = null,
    val url: String? = null,
    @SerialName("created_at") val createdAt: String,
    @SerialName("updated_at") val updatedAt: String,
)

@Serializable
data class ArtifactLink(
    val id: String,
    @SerialName("artifact_id") val artifactId: String,
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    val permission: String,
)

@Serializable
data class LinkArtifactRequest(
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    val permission: String,
)

@Serializable
data class ArtifactAccessRequest(
    val id: String,
    @SerialName("artifact_id") val artifactId: String,
    @SerialName("conversation_id") val conversationId: String,
    val permission: String,
    val reason: String,
    val state: String,
    @SerialName("created_at") val createdAt: String,
    @SerialName("decided_at") val decidedAt: String? = null,
)

@Serializable
data class ApproveArtifactAccessRequest(val project: Boolean = false)

@Serializable
data class PatchArtifactRequest(
    val title: String? = null,
    @SerialName("public") val isPublic: Boolean? = null,
    @SerialName("design_system_id") val designSystemId: String? = null,
)

@Serializable
data class CreateArtifactRequest(
    val kind: String,
    val title: String,
    @SerialName("project_id") val projectId: String? = null,
    @SerialName("design_system_id") val designSystemId: String? = null,
)

object ArtifactKinds {
    const val Design = "design"
    const val Page = "page"
    const val Markdown = "markdown"
    const val File = "file"
    const val Drawing = "drawing"
    const val Typst = "typst"
    const val Doc = "doc"
    const val Slides = "slides"
    const val Dashboard = "dashboard"
    const val Motion = "motion"
    const val DesignSystem = "design_system"
}
