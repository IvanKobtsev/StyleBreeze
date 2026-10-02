import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.intellij.platform.gradle.tasks.PrepareSandboxTask
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.tasks.KotlinCompile

plugins {
    id("org.jetbrains.kotlin.jvm") version "2.3.20"
    id("org.jetbrains.intellij.platform") version "2.18.1"
}

repositories { mavenCentral(); intellijPlatform { defaultRepositories() } }

dependencies {
    intellijPlatform {
        webstorm("2026.2.1")
        bundledPlugin("JavaScript")
        testFramework(TestFrameworkType.Platform)
    }
    testImplementation(kotlin("test"))
}

tasks.withType<KotlinCompile>().configureEach { compilerOptions.jvmTarget = JvmTarget.JVM_25 }

val repositoryRoot = rootProject.layout.projectDirectory.dir("../..")
val executableName = if (System.getProperty("os.name").startsWith("Windows")) "style-breeze.exe" else "style-breeze"
val developmentServer = repositoryRoot.file("target/debug/$executableName")
val buildDevelopmentServer by tasks.registering(Exec::class) {
    workingDir(repositoryRoot)
    commandLine("cargo", "build", "-p", "style-breeze")
}
tasks.named<JavaExec>("runIde") {
    dependsOn(buildDevelopmentServer)
    description = "Build StyleBreeze from source and launch it in an isolated WebStorm instance"
    jvmArgs("-Ddev.stylebreeze.development=true", "-Ddev.stylebreeze.server=${developmentServer.asFile.absolutePath}")
}

intellijPlatform {
    pluginConfiguration { ideaVersion { sinceBuild = "262.9437.145"; untilBuild = "262.*" } }
}

val nativeBinaries = layout.buildDirectory.dir("generated-native-binaries")
val prepareNativeBinaries by tasks.registering(Copy::class) {
    from(rootProject.layout.projectDirectory.dir("../../dist/jetbrains"))
    into(nativeBinaries)
}
tasks.withType<PrepareSandboxTask>().configureEach {
    dependsOn(prepareNativeBinaries)
    from(nativeBinaries) { into(pluginName.map { "$it/bin" }) }
}
val verifyBundledBinaries by tasks.registering {
    dependsOn(tasks.named("buildPlugin"))
    doLast {
        val archive = tasks.named<Zip>("buildPlugin").get().archiveFile.get().asFile
        val entries = mutableSetOf<String>()
        zipTree(archive).visit { if (!isDirectory) entries += relativePath.pathString.replace('\\', '/') }
        mapOf(
            "win32-x64" to "style-breeze.exe", "win32-arm64" to "style-breeze.exe",
            "darwin-x64" to "style-breeze", "darwin-arm64" to "style-breeze",
            "linux-x64" to "style-breeze", "linux-arm64" to "style-breeze",
        ).forEach { (target, binary) ->
            check(entries.any { it.endsWith("/bin/$target/$binary") }) { "Missing bin/$target/$binary" }
        }
    }
}
