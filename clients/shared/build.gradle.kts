import org.jetbrains.kotlin.gradle.ExperimentalWasmDsl

plugins {
    alias(libs.plugins.kotlinMultiplatform)
    alias(libs.plugins.kotlinSerialization)
    alias(libs.plugins.composeCompiler)
    alias(libs.plugins.composeMultiplatform)
    alias(libs.plugins.androidKmpLibrary)
}

val clientVersion = (providers.gradleProperty("appVersion").orNull ?: System.getenv("MYCELIUM_VERSION") ?: System.getenv("CI_COMMIT_TAG") ?: "dev").removePrefix("v")
val buildInfoDir = layout.buildDirectory.dir("generated/buildInfo")
val writeBuildInfo by tasks.registering {
    val version = clientVersion
    val output = buildInfoDir

    inputs.property("version", version)
    outputs.dir(output)
    doLast {
        val file = output.get().file("mycelium/BuildInfo.kt").asFile
        file.parentFile.mkdirs()
        file.writeText("package mycelium\n\nconst val ClientVersion = \"$version\"\n")
    }
}

val changelogDir = layout.buildDirectory.dir("generated/changelog")
val writeChangelog by tasks.registering {
    val source = rootDir.parentFile.resolve("CHANGELOG.md")
    val output = changelogDir

    inputs.files(source).optional()
    outputs.dir(output)
    doLast {
        val file = output.get().file("mycelium/Changelog.kt").asFile
        val lines = if (source.isFile) source.readLines() else emptyList()
        val quoted = lines.joinToString(",\n") { line -> "    \"" + line.replace("\\", "\\\\").replace("\"", "\\\"").replace("$", "\\$") + "\"" }
        file.parentFile.mkdirs()
        file.writeText("package mycelium\n\nval ChangelogText: String = listOf<String>(\n$quoted\n).joinToString(\"\\n\")\n")
    }
}

kotlin {
    jvm()

    android {
        namespace = "mycelium.shared"
        compileSdk = 37
        minSdk = 26
        buildToolsVersion = "37.0.0"
        androidResources.enable = true
    }

    @OptIn(ExperimentalWasmDsl::class)
    wasmJs {
        browser()
    }

    sourceSets {
        commonMain {
            kotlin.srcDir(writeBuildInfo)
            kotlin.srcDir(writeChangelog)
        }
        commonMain.dependencies {
            implementation(libs.compose.runtime)
            implementation(libs.compose.foundation)
            implementation(libs.compose.ui)
            implementation(libs.compose.ui.backhandler)
            implementation(libs.compose.components.resources)
            implementation(libs.ktor.client.core)
            implementation(libs.ktor.client.websockets)
            implementation(libs.ktor.client.content.negotiation)
            implementation(libs.ktor.serialization.json)
            implementation(libs.kotlinx.serialization.json)
            implementation(libs.kotlinx.coroutines.core)
        }
        commonTest.dependencies {
            implementation(libs.kotlin.test)
            implementation(libs.ktor.client.mock)
            implementation(libs.kotlinx.coroutines.test)
        }
        jvmMain.dependencies {
            implementation(libs.ktor.client.cio)
        }
        androidMain.dependencies {
            implementation(libs.ktor.client.cio)
        }
        wasmJsMain.dependencies {
            implementation(libs.ktor.client.js)
        }
    }
}

compose.resources {
    packageOfResClass = "mycelium.resources"
}
