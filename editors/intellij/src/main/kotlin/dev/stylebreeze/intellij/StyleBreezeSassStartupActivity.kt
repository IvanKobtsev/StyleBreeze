package dev.stylebreeze.intellij

import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity

/** Publishes Sass load paths without attaching document-save listeners. */
class StyleBreezeSassStartupActivity : ProjectActivity {
    override suspend fun execute(project: Project) = publishSassSettings(project)
}
