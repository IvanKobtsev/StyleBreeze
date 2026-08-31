package dev.stylebreeze.intellij

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.PrioritizedLookupElement
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.openapi.diagnostic.Logger
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.util.concurrency.AppExecutorUtil
import org.eclipse.lsp4j.CompletionParams
import org.eclipse.lsp4j.Position
import java.util.concurrent.Callable
import java.util.concurrent.TimeUnit

/** Keeps CSS-module completion available in languages without a dedicated contributor. */
class StyleBreezeCompletionContributor : StyleBreezeCompletionContributorBase() {
    override fun accepts(parameters: CompletionParameters): Boolean =
        !parameters.originalFile.virtualFile?.extension.equals("scss", ignoreCase = true)
}

/** Ensures StyleBreeze participates in every SCSS completion session. */
class StyleBreezeScssCompletionContributor : StyleBreezeCompletionContributorBase() {
    override fun accepts(parameters: CompletionParameters): Boolean = true
}

abstract class StyleBreezeCompletionContributorBase : CompletionContributor() {
    protected abstract fun accepts(parameters: CompletionParameters): Boolean

    override fun fillCompletionVariants(parameters: CompletionParameters, result: CompletionResultSet) {
        val file = parameters.originalFile.virtualFile ?: return
        if (!accepts(parameters)) return
        if (!StyleBreezeLspProvider.supports(file)) return
        val document = parameters.editor.document
        val offset = parameters.offset
        if (offset !in 0..document.textLength) return
        val line = document.getLineNumber(offset)
        val position = Position(line, offset - document.getLineStartOffset(line))
        val clients = LspClientManager.getInstance(parameters.originalFile.project)
            .getClients(StyleBreezeLspProvider::class.java)
            .filter { it.descriptor.isSupportedFile(file) }
        log.info("StyleBreeze completion request file=${file.path} offset=$offset clients=${clients.size}")
        for (client in clients) {
            val response = request {
                publishSassSettings(parameters.originalFile.project)
                client.sendRequestSync(2_000) { server ->
                    server.textDocumentService.completion(
                        CompletionParams(client.getDocumentIdentifier(file), position),
                    )
                }
            } ?: continue
            val items = response.left ?: response.right?.items.orEmpty()
            log.info("StyleBreeze completion response file=${file.path} items=${items.size}")
            if (items.isEmpty()) continue
            items.distinctBy { "${it.label}:${it.detail}" }.forEach { item ->
                val lookup = LookupElementBuilder.create(item.label)
                    .withTypeText(item.detail ?: if (item.label.startsWith("--")) "StyleBreeze custom property" else "StyleBreeze CSS Module", true)
                    .withInsertHandler { context, _ ->
                        item.additionalTextEdits.orEmpty()
                            .mapNotNull { edit ->
                                val start = context.document.offset(edit.range.start) ?: return@mapNotNull null
                                val end = context.document.offset(edit.range.end) ?: return@mapNotNull null
                                Triple(start, end, edit.newText)
                            }
                            .sortedByDescending { it.first }
                            .forEach { (start, end, text) -> context.document.replaceString(start, end, text) }
                    }
                result.addElement(PrioritizedLookupElement.withPriority(lookup, PRIORITY))
            }
            // StyleBreeze knows the exact module at this member access, so avoid
            // duplicates and unrelated CSS suggestions from later contributors.
            result.stopHere()
            return
        }
    }

    private fun <T> request(action: () -> T): T? =
        runCatching {
            AppExecutorUtil.getAppExecutorService()
                .submit(Callable(action))
                .get(2_500, TimeUnit.MILLISECONDS)
        }.onFailure {
            log.warn("StyleBreeze completion request failed", it)
        }.getOrNull()

    companion object {
        private const val PRIORITY = 1_000_000.0
        private val log = Logger.getInstance(StyleBreezeCompletionContributorBase::class.java)
    }
}

private fun com.intellij.openapi.editor.Document.offset(position: Position): Int? {
    if (position.line !in 0 until lineCount) return null
    val start = getLineStartOffset(position.line)
    val end = getLineEndOffset(position.line)
    return (start + position.character).coerceIn(start, end)
}
