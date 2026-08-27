package dev.stylebreeze.intellij

import com.intellij.openapi.application.PathManager
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.security.MessageDigest

internal object BundledServer {
    fun executable(): Path {
        val os = System.getProperty("os.name").lowercase()
        val arch = System.getProperty("os.arch").lowercase()
        val platform = when {
            os.contains("win") && arch in setOf("aarch64", "arm64") -> "windows-arm64/stylebreeze.exe"
            os.contains("win") -> "windows-x64/stylebreeze.exe"
            os.contains("mac") && arch.contains("aarch64") -> "macos-arm64/stylebreeze"
            os.contains("mac") -> "macos-x64/stylebreeze"
            arch.contains("aarch64") -> "linux-arm64/stylebreeze"
            else -> "linux-x64/stylebreeze"
        }
        val resource = "/bin/$platform"
        val input = checkNotNull(javaClass.getResourceAsStream(resource)) { "Missing bundled server $resource" }
        val bytes = input.use { it.readBytes() }
        val digest = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
        val target = PathManager.getSystemDir()
            .resolve("plugins")
            .resolve("stylebreeze")
            .resolve(digest)
            .resolve(platform.substringAfterLast('/'))
        if (!Files.exists(target)) {
            Files.createDirectories(target.parent)
            Files.copy(bytes.inputStream(), target, StandardCopyOption.REPLACE_EXISTING)
            target.toFile().setExecutable(true, true)
        }
        return target
    }
}
