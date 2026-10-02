package dev.stylebreeze.jetbrains

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.components.service
import com.intellij.openapi.application.PathManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Key
import com.intellij.openapi.util.SystemInfoRt
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.*
import java.nio.file.Files
import java.nio.file.Path
import org.eclipse.lsp4j.jsonrpc.services.JsonNotification

class StyleBreezeLspIntegrationProvider : LspIntegrationProvider {
    override fun fileOpened(project: Project, file: VirtualFile, clientStarter: LspIntegrationProvider.LspClientStarter) {
        val settings = StyleBreezeSettings.getInstance(project)
        settings.initialize(project)
        if (!settings.state.showDiagnostics || file.extension?.lowercase() !in extensions) return
        project.service<StyleBreezeIssueNotifier>().checkConfiguration()
        if (settings.configPath(project)?.toFile()?.isFile != true || executable() == null) return
        clientStarter.ensureClientStarted(descriptor(project))
    }

    companion object {
        private val extensions = setOf("ts", "tsx", "css", "scss")
        private val key = Key.create<Descriptor>("dev.stylebreeze.jetbrains.lspDescriptor")
        private fun descriptor(project: Project) = project.getUserData(key) ?: synchronized(project) {
            project.getUserData(key) ?: Descriptor(project).also { project.putUserData(key, it) }
        }

        private fun executable(): Path? {
            System.getProperty("dev.stylebreeze.server")?.takeIf(String::isNotBlank)?.let { return Path.of(it) }
            val architecture = if (System.getProperty("os.arch").lowercase() in setOf("aarch64", "arm64")) "arm64" else "x64"
            val platform = when {
                SystemInfoRt.isWindows -> "win32-$architecture"
                SystemInfoRt.isMac -> "darwin-$architecture"
                else -> "linux-$architecture"
            }
            val name = if (SystemInfoRt.isWindows) "style-breeze.exe" else "style-breeze"
            val relative = Path.of("bin", platform, name)
            val location = runCatching { Path.of(StyleBreezeLspIntegrationProvider::class.java.protectionDomain.codeSource.location.toURI()) }.getOrNull()
            var root = location?.let { if (Files.isRegularFile(it)) it.parent else it }
            repeat(4) {
                if (root != null && Files.isDirectory(root!!.resolve("bin"))) return@repeat
                root = root?.parent
            }
            val path = sequenceOf(
                root?.resolve(relative),
                Path.of(PathManager.getPluginsPath()).resolve("style-breeze-jetbrains").resolve(relative),
            ).filterNotNull().firstOrNull(Files::isRegularFile) ?: return null
            if (!Files.isRegularFile(path)) return null
            if (!SystemInfoRt.isWindows) path.toFile().setExecutable(true)
            return path
        }

        private class Descriptor(project: Project) : ProjectWideLspClientDescriptor(project, "StyleBreeze") {
            override fun isSupportedFile(file: VirtualFile) = StyleBreezeSettings.getInstance(project).state.showDiagnostics && file.extension?.lowercase() in extensions
            override fun createCommandLine(): GeneralCommandLine {
                val config = checkNotNull(StyleBreezeSettings.getInstance(project).configPath(project))
                val command = GeneralCommandLine(checkNotNull(executable()).toString(), "lsp", "--stdio", "--config", config.toString())
                project.basePath?.let(command::withWorkDirectory)
                return command
            }
            override fun createLsp4jClient(handler: LspServerNotificationsHandler): Lsp4jClient = Client(handler, project)
        }

        private class Client(handler: LspServerNotificationsHandler, private val project: Project) : Lsp4jClient(handler) {
            @JsonNotification("styleBreeze/workspaceIssue")
            fun workspaceIssue(issue: WorkspaceIssue) = project.service<StyleBreezeIssueNotifier>().workspaceIssue(issue)

        }
    }
}
