plugins {
    kotlin("jvm") version "2.2.20"
    id("org.jetbrains.intellij.platform") version "2.9.0"
}

group = "dev.stylebreeze"
version = "0.1.0"

repositories {
    mavenCentral()
    intellijPlatform { defaultRepositories() }
}

dependencies {
    intellijPlatform { webstorm("2026.2") }
}

kotlin { jvmToolchain(21) }

intellijPlatform {
    pluginConfiguration {
        ideaVersion { sinceBuild = "262" }
    }
}

tasks.test { useJUnitPlatform() }

