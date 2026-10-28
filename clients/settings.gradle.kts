pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

plugins {
    id("com.github.burrunan.s3-build-cache") version "1.9.10"
}

data class RemoteBuildCache(val endpoint: String, val accessKey: String, val secretKey: String)

val remoteBuildCache: RemoteBuildCache? = run {
    val file = File(System.getenv("BUILD_CACHE_CREDENTIALS") ?: "${System.getProperty("user.home")}/.config/build-cache/credentials")
    val fields = file.takeIf { it.isFile }?.readLines()?.mapNotNull { line -> line.split("=", limit = 2).takeIf { it.size == 2 }?.let { it[0].trim() to it[1].trim() } }?.toMap().orEmpty()
    val endpoint = System.getenv("BUILD_CACHE_ENDPOINT") ?: fields["endpoint"]
    val accessKey = System.getenv("BUILD_CACHE_ACCESS_KEY") ?: fields["aws_access_key_id"]
    val secretKey = System.getenv("BUILD_CACHE_SECRET_KEY") ?: fields["aws_secret_access_key"]
    if (endpoint != null && accessKey != null && secretKey != null) RemoteBuildCache(endpoint, accessKey, secretKey) else null
}

buildCache {
    local {
        isEnabled = true
    }

    if (remoteBuildCache != null) {
        remote<com.github.burrunan.s3cache.AwsS3BuildCache> {
            endpoint = remoteBuildCache.endpoint
            region = "garage"
            bucket = "build-cache"
            prefix = "mycelium/gradle/"
            forcePathStyle = true
            awsAccessKeyId = remoteBuildCache.accessKey
            awsSecretKey = remoteBuildCache.secretKey
            isPush = true
        }
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "mycelium-clients"

include(":shared", ":web", ":desktop", ":androidApp")
