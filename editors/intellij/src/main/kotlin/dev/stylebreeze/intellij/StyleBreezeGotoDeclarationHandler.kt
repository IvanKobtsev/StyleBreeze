package dev.stylebreeze.intellij

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.util.SystemInfoRt
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspClient
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiManager
import com.intellij.util.concurrency.AppExecutorUtil
import org.eclipse.lsp4j.DefinitionParams
import org.eclipse.lsp4j.Position
import org.eclipse.lsp4j.ReferenceContext
import org.eclipse.lsp4j.ReferenceParams
import java.util.concurrent.Callable
import java.util.concurrent.TimeUnit

/**
 * Gives StyleBreeze an authoritative Ctrl+Click path when WebStorm's built-in
 * CSS/TypeScript providers would otherwise suppress or replace the LSP result.
 */
class StyleBreezeGotoDeclarationHandler : GotoDeclarationHandler {
    override fun getGotoDeclarationTargets(
        sourceElement: PsiElement?,
        offset: Int,
        editor: Editor?,
    ): Array<PsiElement>? {
        val element = sourceElement ?: return null
        val actualEditor = editor ?: return null
        val file = element.containingFile?.virtualFile ?: return null
        if (!StyleBreezeLspProvider.supports(file)) return null
        val document = actualEditor.document
        if (offset !in 0..document.textLength) return null
        val line = document.getLineNumber(offset)
        val position = Position(line, offset - document.getLineStartOffset(line))
        val clients = LspClientManager.getInstance(element.project)
            .getClients(StyleBreezeLspProvider::class.java)
            .filter { it.descriptor.isSupportedFile(file) }

        for (client in clients) {
            if (isStylesheet(file)) {
                val references = request("references") {
                    client.sendRequestSync(2_000) { server ->
                        server.textDocumentService.references(
                            ReferenceParams(
                                client.getDocumentIdentifier(file),
                                position,
                                ReferenceContext(false),
                            ),
                        )
                    }
                }.orEmpty()
                val targets = mapTargets(
                    client,
                    references.map { Target(it.uri, it.range) },
                ).filterNot { it.containingFile?.virtualFile == file }
                if (targets.isNotEmpty()) return targets.toTypedArray()
                continue
            }

            val definitions = request("definition") {
                client.sendRequestSync(2_000) { server ->
                    server.textDocumentService.definition(
                        DefinitionParams(client.getDocumentIdentifier(file), position),
                    )
                }
            }
            val targets = buildList {
                definitions?.left?.forEach { add(Target(it.uri, it.range)) }
                definitions?.right?.forEach { add(Target(it.targetUri, it.targetSelectionRange)) }
            }
            mapTargets(client, targets).takeIf { it.isNotEmpty() }?.let {
                return it.toTypedArray()
            }
        }
        return null
    }

    override fun getActionText(context: DataContext): String? = null

    private fun <T> request(operation: String, action: () -> T): T? =
        runCatching {
            AppExecutorUtil.getAppExecutorService()
                .submit(Callable(action))
                .get(2_500, TimeUnit.MILLISECONDS)
        }.onFailure {
            log.warn("StyleBreeze $operation navigation request failed", it)
        }.getOrNull()

    private fun mapTargets(client: LspClient, targets: List<Target>): List<PsiElement> {
        val psiManager = PsiManager.getInstance(client.project)
        val seen = mutableSetOf<String>()
        return targets.mapNotNull { target ->
            val virtualFile = client.descriptor.findFileByUri(target.uri) ?: return@mapNotNull null
            val identity = "${virtualFile.identity()}:${target.range.start.line}:${target.range.start.character}"
            if (!seen.add(identity)) return@mapNotNull null
            val psiFile = psiManager.findFile(virtualFile) ?: return@mapNotNull null
            val targetDocument = FileDocumentManager.getInstance().getDocument(virtualFile) ?: return@mapNotNull null
            val targetOffset = targetDocument.offset(target.range.start) ?: return@mapNotNull null
            psiFile.findElementAt(targetOffset) ?: psiFile
        }
    }

    companion object {
        private val log = Logger.getInstance(StyleBreezeGotoDeclarationHandler::class.java)

        private fun isStylesheet(file: VirtualFile): Boolean {
            val name = file.name.lowercase()
            return name.endsWith(".module.css") || name.endsWith(".module.scss")
        }
    }
}

private data class Target(val uri: String, val range: org.eclipse.lsp4j.Range)

private fun VirtualFile.identity(): String = if (SystemInfoRt.isWindows) path.lowercase() else path

private fun com.intellij.openapi.editor.Document.offset(position: Position): Int? {
    if (position.line !in 0 until lineCount) return null
    val start = getLineStartOffset(position.line)
    val end = getLineEndOffset(position.line)
    return (start + position.character).coerceIn(start, end)
}
