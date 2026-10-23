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

val buildCacheKeys: Pair<String, String>? = run {
    val fromCi = System.getenv("BUILD_CACHE_ACCESS_KEY")?.let { key -> System.getenv("BUILD_CACHE_SECRET_KEY")?.let { secret -> key to secret } }
    val file = File(System.getenv("BUILD_CACHE_CREDENTIALS") ?: "${System.getProperty("user.home")}/.config/build-cache/credentials")
    val fromFile = file.takeIf { it.isFile }?.readLines()?.mapNotNull { line -> line.split("=", limit = 2).takeIf { it.size == 2 }?.let { it[0].trim() to it[1].trim() } }?.toMap()?.let { fields ->
        fields["aws_access_key_id"]?.let { key -> fields["aws_secret_access_key"]?.let { secret -> key to secret } }
    }

    fromCi ?: fromFile
}

buildCache {
    local {
        isEnabled = true
    }

    if (buildCacheKeys != null) {
        remote<com.github.burrunan.s3cache.AwsS3BuildCache> {
            endpoint = "https://s3.example.com"
            region = "garage"
            bucket = "build-cache"
            prefix = "mycelium/gradle/"
            forcePathStyle = true
            awsAccessKeyId = buildCacheKeys.first
            awsSecretKey = buildCacheKeys.second
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
