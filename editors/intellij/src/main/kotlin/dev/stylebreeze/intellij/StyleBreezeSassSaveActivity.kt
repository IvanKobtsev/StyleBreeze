package dev.stylebreeze.intellij

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.FileDocumentManagerListener
import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity
import com.intellij.platform.lsp.api.LspClientManager
import org.eclipse.lsp4j.Position

class StyleBreezeSassSaveActivity : ProjectActivity {
    override suspend fun execute(project: Project) {
        publishSassSettings(project)
        project.messageBus.connect().subscribe(FileDocumentManagerListener.TOPIC, object : FileDocumentManagerListener {
            override fun beforeDocumentSaving(document: Document) {
                val settings = project.getService(StyleBreezeSassSettings::class.java).state
                if (!settings.fixImportsOnSave) return
                val file = FileDocumentManager.getInstance().getFile(document) ?: return
                if (!file.name.lowercase().endsWith(".scss")) return
                val client = LspClientManager.getInstance(project).getClients(StyleBreezeLspProvider::class.java)
                    .firstOrNull { it.descriptor.isSupportedFile(file) } ?: return
                publishSassSettings(project)
                val response = runCatching {
                    client.sendRequestSync(2_000) { server ->
                        (server as StyleBreezeLanguageServer).fixSassImports(client.getDocumentIdentifier(file))
                    }
                }.onFailure { log.warn("StyleBreeze SCSS import fixing failed", it) }.getOrNull() ?: return
                val edits = response.edits.mapNotNull { edit ->
                    val start = document.offset(edit.range.start) ?: return@mapNotNull null
                    val end = document.offset(edit.range.end) ?: return@mapNotNull null
                    Triple(start, end, edit.newText)
                }.sortedByDescending { it.first }
                if (edits.isEmpty()) return
                val apply = Runnable { edits.forEach { (start, end, text) -> document.replaceString(start, end, text) } }
                if (ApplicationManager.getApplication().isWriteAccessAllowed) apply.run()
                else ApplicationManager.getApplication().runWriteAction(apply)
            }
        })
    }

    companion object { private val log = Logger.getInstance(StyleBreezeSassSaveActivity::class.java) }
}

private fun Document.offset(position: Position): Int? {
    if (position.line !in 0 until lineCount) return null
    val start = getLineStartOffset(position.line)
    val end = getLineEndOffset(position.line)
    return (start + position.character).coerceIn(start, end)
}
