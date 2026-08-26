package dev.stylebreeze.intellij

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspIntegrationProvider
import com.intellij.platform.lsp.api.ProjectWideLspClientDescriptor

internal class StyleBreezeLspProvider : LspIntegrationProvider {
    override fun fileOpened(
        project: Project,
        file: VirtualFile,
        clientStarter: LspIntegrationProvider.LspClientStarter,
    ) {
        if (StyleBreezeDescriptor.supports(file)) {
            clientStarter.ensureClientStarted(StyleBreezeDescriptor(project))
        }
    }
}

private class StyleBreezeDescriptor(project: Project) :
    ProjectWideLspClientDescriptor(project, "StyleBreeze") {
    override fun isSupportedFile(file: VirtualFile): Boolean = supports(file)

    override fun createCommandLine(): GeneralCommandLine =
        GeneralCommandLine(BundledServer.executable().toString(), "--stdio")

    companion object {
        fun supports(file: VirtualFile): Boolean {
            val name = file.name.lowercase()
            return name.endsWith(".module.css") || name.endsWith(".module.scss") ||
                file.extension?.lowercase() in setOf("js", "jsx", "ts", "tsx")
        }
    }
}
