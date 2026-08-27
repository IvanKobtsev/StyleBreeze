package dev.stylebreeze.intellij

import com.intellij.codeInsight.daemon.LineMarkerInfo
import com.intellij.codeInsight.daemon.LineMarkerProvider
import com.intellij.openapi.editor.markup.GutterIconRenderer
import com.intellij.openapi.util.IconLoader
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.psi.PsiElement
import com.intellij.util.concurrency.AppExecutorUtil
import org.eclipse.lsp4j.HoverParams
import org.eclipse.lsp4j.Position
import java.util.concurrent.Callable
import java.util.concurrent.TimeUnit

class StyleBreezeModifierLineMarkerProvider : LineMarkerProvider {
    override fun getLineMarkerInfo(element: PsiElement): LineMarkerInfo<*>? = null

    override fun collectSlowLineMarkers(
        elements: List<PsiElement>,
        result: MutableCollection<in LineMarkerInfo<*>>,
    ) {
        elements.mapNotNull(::modifierMarker).forEach(result::add)
    }

    private fun modifierMarker(element: PsiElement): LineMarkerInfo<*>? {
        if (element.firstChild != null || !element.text.matches(IDENTIFIER)) return null
        val file = element.containingFile?.virtualFile ?: return null
        if (!file.name.lowercase().let { it.endsWith(".module.css") || it.endsWith(".module.scss") }) return null
        val document = element.containingFile.viewProvider.document ?: return null
        val start = element.textRange.startOffset
        if (start <= 0 || document.charsSequence[start - 1] != '.') return null
        val line = document.getLineNumber(start)
        val position = Position(line, start - document.getLineStartOffset(line))
        val clients = LspClientManager.getInstance(element.project)
            .getClients(StyleBreezeLspProvider::class.java)
            .filter { it.descriptor.isSupportedFile(file) }
        val tooltip = clients.firstNotNullOfOrNull { client ->
            runCatching {
                AppExecutorUtil.getAppExecutorService().submit(Callable {
                    client.sendRequestSync(1_000) { server ->
                        server.textDocumentService.hover(HoverParams(client.getDocumentIdentifier(file), position))
                    }
                }).get(1_250, TimeUnit.MILLISECONDS)
            }.getOrNull()?.contents?.let { contents ->
                contents.right?.value ?: contents.left?.joinToString("\n") { it.left ?: it.right?.value.orEmpty() }
            }
        } ?: return null
        return LineMarkerInfo(
            element,
            element.textRange,
            ICON,
            { tooltip },
            null,
            GutterIconRenderer.Alignment.LEFT,
            { "CSS dependent modifier" },
        )
    }

    companion object {
        private val IDENTIFIER = Regex("[A-Za-z_-][A-Za-z0-9_-]*")
        private val ICON = IconLoader.getIcon("/icons/modifierChain.svg", StyleBreezeModifierLineMarkerProvider::class.java)
    }
}
