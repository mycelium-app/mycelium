package mycelium.api

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class AppsCredentialRequest(val url: String, val token: String)

@Serializable
data class CoolifyProject(val uuid: String, val name: String)

@Serializable
data class CoolifyServer(val uuid: String, val name: String, @SerialName("wildcard_domain") val wildcardDomain: String? = null)

@Serializable
data class CoolifyOptions(val projects: List<CoolifyProject> = emptyList(), val servers: List<CoolifyServer> = emptyList())

@Serializable
data class CoolifyTested(
    val host: String,
    val version: String? = null,
    val projects: List<CoolifyProject> = emptyList(),
    val servers: List<CoolifyServer> = emptyList(),
)

@Serializable
data class CoolifyTargetRequest(@SerialName("project_uuid") val projectUuid: String? = null, @SerialName("server_uuid") val serverUuid: String? = null)

@Serializable
data class TandoorTested(val host: String, val space: String, val user: String, val recipes: Int = 0)

@Serializable
data class AppSummary(
    val uuid: String,
    val name: String,
    val status: String = "stopped",
    val address: String? = null,
    @SerialName("last_deploy_at") val lastDeployAt: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    val repository: String? = null,
    val branch: String? = null,
)

@Serializable
data class AppList(val apps: List<AppSummary> = emptyList())

@Serializable
data class AppDeployment(
    val uuid: String,
    @SerialName("started_at") val startedAt: String,
    val commit: String? = null,
    @SerialName("commit_message") val commitMessage: String? = null,
    @SerialName("triggered_by") val triggeredBy: String = "unknown",
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("duration_seconds") val durationSeconds: Long? = null,
    val result: String = "deploying",
)

@Serializable
data class AppDetail(
    val uuid: String,
    val name: String,
    val status: String = "stopped",
    val address: String? = null,
    val repository: String? = null,
    val branch: String? = null,
    @SerialName("base_directory") val baseDirectory: String? = null,
    @SerialName("build_pack") val buildPack: String? = null,
    val project: String? = null,
    val server: String? = null,
    @SerialName("conversation_id") val conversationId: String? = null,
    @SerialName("created_at") val createdAt: String? = null,
    @SerialName("latest_deployment") val latestDeployment: AppDeployment? = null,
)

@Serializable
data class AppLogs(val lines: List<String> = emptyList(), val available: Boolean = true, val message: String? = null)

@Serializable
data class AppEnv(
    val key: String,
    val secret: Boolean = false,
    @SerialName("build_time") val buildTime: Boolean = false,
    val value: String? = null,
    @SerialName("changed_at") val changedAt: String? = null,
    @SerialName("changed_by") val changedBy: String? = null,
    val stale: Boolean = false,
)

@Serializable
data class PutAppEnvRequest(val value: String, val secret: Boolean, @SerialName("build_time") val buildTime: Boolean)

@Serializable
data class AppRedeployed(@SerialName("deployment_uuid") val deploymentUuid: String)

@Serializable
data class DeploymentStep(val label: String, val state: String = "pending", @SerialName("duration_seconds") val durationSeconds: Long? = null)

@Serializable
data class DeploymentProgress(
    val uuid: String,
    @SerialName("app_uuid") val appUuid: String? = null,
    @SerialName("app_name") val appName: String = "",
    val status: String = "deploying",
    val steps: List<DeploymentStep> = emptyList(),
    val commit: String? = null,
    val address: String? = null,
    @SerialName("log_tail") val logTail: List<String> = emptyList(),
)
