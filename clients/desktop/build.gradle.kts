import java.net.URI
import java.security.MessageDigest
import org.jetbrains.compose.desktop.application.dsl.TargetFormat

plugins {
    alias(libs.plugins.kotlinJvm)
    alias(libs.plugins.kotlinSerialization)
    alias(libs.plugins.composeCompiler)
    alias(libs.plugins.composeMultiplatform)
}

val wakeDirectory = layout.buildDirectory.dir("wake")

val wakeDownloads = mapOf(
    "sherpa-onnx-jvm-2.0.1.jar" to Pair(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-jvm-2.0.1.jar",
        "8320e2457b6e38dd624fb619d3552e11ef78ffeb394622056126b16f2fd99d12",
    ),
    "sherpa-onnx-native-lib-linux-x64-2.0.1.jar" to Pair(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-native-lib-linux-x64-2.0.1.jar",
        "744330a68a7024d9412f029c23b06f847b8444787c94e5118e0fdaf1b4b2cb3f",
    ),
    "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2" to Pair(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2",
        "f170013b4716e41b62b9bfd809687c207cef798ef9bc6534d524e17af9b6561a",
    ),
)

val downloadWakeAssets = tasks.register("downloadWakeAssets") {
    val downloads = wakeDirectory.map { it.dir("downloads") }
    val assets = wakeDownloads.toMap()

    inputs.property("assets", assets.toString())
    outputs.dir(downloads)

    doLast {
        val directory = downloads.get().asFile.also { it.mkdirs() }

        assets.forEach { (name, source) ->
            val target = File(directory, name)
            val sha256 = { file: File -> MessageDigest.getInstance("SHA-256").digest(file.readBytes()).joinToString("") { "%02x".format(it) } }

            if (!target.exists() || sha256(target) != source.second) {
                val partial = File(directory, "$name.partial")

                URI(source.first).toURL().openStream().use { input -> partial.outputStream().use { input.copyTo(it) } }

                check(sha256(partial) == source.second) { "$name does not match its pinned checksum" }
                partial.renameTo(target)
            }
        }
    }
}

val prepareWakeModel = tasks.register<Sync>("prepareWakeModel") {
    dependsOn(downloadWakeAssets)
    from(tarTree(resources.bzip2(wakeDirectory.map { it.file("downloads/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2") }))) {
        include("*/encoder-*.int8.onnx", "*/decoder-*.int8.onnx", "*/joiner-*.int8.onnx", "*/tokens.txt")
        eachFile { relativePath = RelativePath(true, name) }
    }
    from("src/main/wake/keywords.txt")
    includeEmptyDirs = false
    into(wakeDirectory.map { it.dir("resources/common/kws") })
}

dependencies {
    implementation(project(":shared"))
    implementation(files(wakeDirectory.map { it.file("downloads/sherpa-onnx-jvm-2.0.1.jar") }, wakeDirectory.map { it.file("downloads/sherpa-onnx-native-lib-linux-x64-2.0.1.jar") }).builtBy(downloadWakeAssets))
    implementation(libs.compose.desktop)
    implementation(libs.kotlinx.coroutines.swing)
    implementation(libs.kotlinx.serialization.json)

    testImplementation(libs.kotlin.test)
    testImplementation(libs.compose.ui.test)
    testImplementation(libs.ktor.client.mock)
    testImplementation(libs.ktor.client.cio)
    testImplementation(libs.kotlinx.coroutines.test)
}

val jcefOpens = listOf(
    "--add-opens=java.desktop/sun.awt=ALL-UNNAMED",
    "--add-opens=java.desktop/java.awt.peer=ALL-UNNAMED",
    "--add-opens=java.desktop/sun.lwawt=ALL-UNNAMED",
    "--add-opens=java.desktop/sun.lwawt.macosx=ALL-UNNAMED",
)

tasks.test {
    dependsOn(prepareWakeModel)
    systemProperty("mycelium.wakeModel", wakeDirectory.get().dir("resources/common/kws").asFile.path)
    useJUnit()
    jvmArgs(jcefOpens)
    maxParallelForks = (Runtime.getRuntime().availableProcessors() / 2).coerceAtLeast(1)
}

compose.desktop {
    application {
        mainClass = "mycelium.desktop.MainKt"
        jvmArgs(*jcefOpens.toTypedArray())

        nativeDistributions {
            appResourcesRootDir.set(wakeDirectory.map { it.dir("resources") })
            targetFormats(TargetFormat.Deb, TargetFormat.Dmg, TargetFormat.Msi)
            packageName = "mycelium"
            packageVersion = System.getenv("MYCELIUM_VERSION")?.removePrefix("v") ?: "1.0.0"
            System.getenv("MYCELIUM_JBR_HOME")?.let { javaHome = it }
            modules("jcef", "jdk.unsupported", "java.desktop")
        }
    }
}

tasks.configureEach {
    if (name == "prepareAppResources") {
        dependsOn(prepareWakeModel)
    }
}
