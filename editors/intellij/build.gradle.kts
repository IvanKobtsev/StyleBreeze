import dev.stylebreeze.gradle.VerifyPluginBinaries

plugins {
    kotlin("jvm") version "2.4.0"
    id("org.jetbrains.intellij.platform") version "2.18.1"
}

group = "dev.stylebreeze"
version = "0.3.0"

repositories {
    mavenCentral()
    intellijPlatform { defaultRepositories() }
}

dependencies {
    intellijPlatform { webstorm("2026.2") }
}

kotlin { jvmToolchain(21) }

intellijPlatform {
    // StyleBreeze has no GUI Designer forms and Kotlin already emits its null
    // checks, so JetBrains bytecode instrumentation is unnecessary.
    instrumentCode = false
    pluginConfiguration {
        ideaVersion { sinceBuild = "262" }
    }
}

tasks.test { useJUnitPlatform() }

val buildPlugin = tasks.named<Zip>("buildPlugin")

tasks.register<VerifyPluginBinaries>("verifyBundledBinaries") {
    dependsOn(buildPlugin)
    pluginArchive.set(buildPlugin.flatMap { it.archiveFile })
}
