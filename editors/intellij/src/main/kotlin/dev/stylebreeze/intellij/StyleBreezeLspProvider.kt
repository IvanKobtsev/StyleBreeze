package dev.stylebreeze.intellij

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspIntegrationProvider
import com.intellij.platform.lsp.api.ProjectWideLspClientDescriptor
import com.intellij.platform.lsp.api.customization.LspCustomization
import com.intellij.platform.lsp.api.customization.LspGoToDefinitionDisabled

class StyleBreezeLspProvider : LspIntegrationProvider {
    override fun fileOpened(
        project: Project,
        file: VirtualFile,
        clientStarter: LspIntegrationProvider.LspClientStarter,
    ) {
        if (supports(file)) {
            clientStarter.ensureClientStarted(StyleBreezeDescriptor(project))
        }
    }

    companion object {
        internal fun supports(file: VirtualFile): Boolean {
            val name = file.name.lowercase()
            return name.endsWith(".module.css") || name.endsWith(".module.scss") ||
                file.extension?.lowercase() in setOf("js", "jsx", "ts", "tsx")
        }
    }
}

private class StyleBreezeDescriptor(project: Project) :
    ProjectWideLspClientDescriptor(project, "StyleBreeze") {
    override fun isSupportedFile(file: VirtualFile): Boolean = StyleBreezeLspProvider.supports(file)

    override fun createCommandLine(): GeneralCommandLine =
        GeneralCommandLine(BundledServer.executable().toString(), "--stdio")

    override val lspCustomization: LspCustomization = StyleBreezeLspCustomization

}

private object StyleBreezeLspCustomization : LspCustomization() {
    override val goToDefinitionCustomizer = LspGoToDefinitionDisabled
}
