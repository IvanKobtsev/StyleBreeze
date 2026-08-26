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

/** Makes resolved StyleBreeze exports authoritative in a `styles.` completion. */
class StyleBreezeCompletionContributor : CompletionContributor() {
    override fun fillCompletionVariants(parameters: CompletionParameters, result: CompletionResultSet) {
        val file = parameters.originalFile.virtualFile ?: return
        if (!StyleBreezeLspProvider.supports(file)) return
        val document = parameters.editor.document
        val offset = parameters.offset
        if (offset !in 0..document.textLength) return
        val line = document.getLineNumber(offset)
        val position = Position(line, offset - document.getLineStartOffset(line))
        val clients = LspClientManager.getInstance(parameters.originalFile.project)
            .getClients(StyleBreezeLspProvider::class.java)
            .filter { it.descriptor.isSupportedFile(file) }

        for (client in clients) {
            val response = request {
                client.sendRequestSync(2_000) { server ->
                    server.textDocumentService.completion(
                        CompletionParams(client.getDocumentIdentifier(file), position),
                    )
                }
            } ?: continue
            val items = response.left ?: response.right?.items.orEmpty()
            if (items.isEmpty()) continue
            items.distinctBy { it.label }.forEach { item ->
                val lookup = LookupElementBuilder.create(item.label)
                    .withTypeText("StyleBreeze CSS Module", true)
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
        private val log = Logger.getInstance(StyleBreezeCompletionContributor::class.java)
    }
}
