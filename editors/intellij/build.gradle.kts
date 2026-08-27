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

val verifyBundledBinaries by tasks.registering {
    dependsOn(tasks.named("buildPlugin"))
    doLast {
        val archive = tasks.named<Zip>("buildPlugin").get().archiveFile.get().asFile
        val entries = mutableSetOf<String>()
        zipTree(archive).visit {
            if (!isDirectory) entries += relativePath.pathString.replace('\\', '/')
        }
        val expected = mapOf(
            "windows-x64" to "stylebreeze.exe",
            "windows-arm64" to "stylebreeze.exe",
            "macos-x64" to "stylebreeze",
            "macos-arm64" to "stylebreeze",
            "linux-x64" to "stylebreeze",
            "linux-arm64" to "stylebreeze",
        )
        for ((platform, binary) in expected) {
            check(entries.any { it.endsWith("/bin/$platform/$binary") }) {
                "Plugin archive is missing executable: bin/$platform/$binary"
            }
        }
    }
}
