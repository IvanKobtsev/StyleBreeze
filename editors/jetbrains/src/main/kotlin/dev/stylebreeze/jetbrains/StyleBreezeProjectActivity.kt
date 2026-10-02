package dev.stylebreeze.jetbrains

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import com.intellij.platform.lsp.api.LspClientManager

class StyleBreezeProjectActivity : ProjectActivity {
    override suspend fun execute(project: Project) {
        val settings = StyleBreezeSettings.getInstance(project)
        settings.initialize(project)
        project.service<StyleBreezeIssueNotifier>().checkConfiguration()
        project.messageBus.connect(project).subscribe(VirtualFileManager.VFS_CHANGES, object : BulkFileListener {
            override fun after(events: List<VFileEvent>) {
                val config = settings.configPath(project)?.toString() ?: return
                val relevant = events.any {
                    it.path.equals(config, ignoreCase = System.getProperty("os.name").startsWith("Windows")) ||
                        it.path.endsWith("/tsconfig.json") ||
                        it.path.substringAfterLast('.').lowercase() in setOf("ts", "tsx", "css", "scss")
                }
                if (!relevant) return
                ApplicationManager.getApplication().invokeLater {
                    if (!project.isDisposed) {
                        project.service<StyleBreezeIssueNotifier>().checkConfiguration()
                        LspClientManager.getInstance(project).stopAndRestartClientsIfNeeded(StyleBreezeLspIntegrationProvider::class.java)
                    }
                }
            }
        })
    }
}
