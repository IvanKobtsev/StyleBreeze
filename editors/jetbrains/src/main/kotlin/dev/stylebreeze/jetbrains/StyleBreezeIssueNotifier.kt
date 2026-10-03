package dev.stylebreeze.jetbrains

import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.components.Service
import com.intellij.openapi.project.Project

@Service(Service.Level.PROJECT)
class StyleBreezeIssueNotifier(private val project: Project) {
    private var lastIssue: String? = null

    fun checkConfiguration() {
        val settings = StyleBreezeSettings.getInstance(project)
        if (!settings.state.showDiagnostics) { lastIssue = null; return }
        val path = settings.configPath(project)
        if (path != null && path.toFile().isFile && path.toFile().canRead()) { lastIssue = null; return }
        notifyOnce("StyleContract configuration was not found or is unreadable: ${path ?: "no path selected"}")
    }

    fun workspaceIssue(issue: WorkspaceIssue) {
        if (project.isDisposed) return
        if (!issue.active) { lastIssue = null; return }
        // Parse failures in an edited TS/CSS/SCSS document are transient editor
        // state. The server already clears diagnostics for that document, so a
        // project-wide notification would only turn normal typing into noise.
        // Missing configuration is handled locally by checkConfiguration(); an
        // invalid existing configuration is reported on the config file itself.
    }

    private fun notifyOnce(message: String) {
        if (lastIssue == message) return
        lastIssue = message
        NotificationGroupManager.getInstance().getNotificationGroup("StyleBreeze")
            .createNotification("StyleBreeze diagnostics unavailable", message, NotificationType.WARNING)
            .notify(project)
    }
}

data class WorkspaceIssue(val version: Int = 1, val active: Boolean = true, val code: String = "", val message: String = "", val path: String? = null)
