package dev.stylebreeze.intellij

import com.intellij.lang.documentation.AbstractDocumentationProvider
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.util.text.StringUtil
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.psi.PsiElement
import com.intellij.ui.ColorUtil
import com.intellij.ui.JBColor
import org.eclipse.lsp4j.Position
import org.eclipse.lsp4j.TextDocumentPositionParams

class StyleBreezeSelectorDocumentationProvider : AbstractDocumentationProvider() {
    override fun generateDoc(element: PsiElement, originalElement: PsiElement?): String? =
        selectorDocumentation(originalElement ?: element)

    override fun generateHoverDoc(element: PsiElement, originalElement: PsiElement?): String? =
        selectorDocumentation(originalElement ?: element)

    private fun selectorDocumentation(element: PsiElement): String? {
        val psiFile = element.containingFile ?: return null
        val file = psiFile.virtualFile ?: return null
        if (!file.name.lowercase().let { it.endsWith(".module.css") || it.endsWith(".module.scss") }) return null
        val document = psiFile.viewProvider.document ?: return null
        val offset = element.textRange.startOffset.coerceIn(0, document.textLength)
        val line = document.getLineNumber(offset)
        val params = TextDocumentPositionParams().apply {
            position = Position(line, offset - document.getLineStartOffset(line))
        }
        val clients = LspClientManager.getInstance(element.project)
            .getClients(StyleBreezeLspProvider::class.java)
            .filter { it.descriptor.isSupportedFile(file) }
        for (client in clients) {
            params.textDocument = client.getDocumentIdentifier(file)
            val response = runCatching {
                client.sendRequestSync(2_000) { server ->
                    (server as StyleBreezeLanguageServer).selectorPreview(params)
                }
            }.onFailure { log.warn("StyleBreeze selector preview request failed", it) }.getOrNull() ?: continue
            response.unsupported?.let {
                return "<div class='definition'><b>Selector preview unavailable</b></div>" +
                    "<div class='content'>${escape(it.message)}</div>"
            }
            response.preview?.let { return render(it) }
        }
        return null
    }

    private fun render(preview: SelectorPreview): String {
        val selectedBackground = ColorUtil.toHex(
            JBColor.namedColor("StyleBreeze.SelectorPreview.selectedBackground", JBColor(0xDCEEFF, 0x123B5D)),
        )
        val selectedBorder = ColorUtil.toHex(
            JBColor.namedColor("StyleBreeze.SelectorPreview.selectedBorder", JBColor(0x3578C9, 0x5CA9FF)),
        )
        val witnessBackground = ColorUtil.toHex(
            JBColor.namedColor("StyleBreeze.SelectorPreview.witnessBackground", JBColor(0xF0F3F7, 0x30343A)),
        )
        val nodesByParent = preview.nodes.groupBy { it.parent }
        val body = StringBuilder()
        fun appendNode(node: PreviewNode, depth: Int) {
            val indent = "&nbsp;".repeat(depth * 4)
            val attributes = buildString {
                node.elementId?.let { append(" id=\"").append(escape(it)).append("\"") }
                if (node.classes.isNotEmpty()) append(" class=\"").append(node.classes.joinToString(" ", transform = ::escape)).append("\"")
                node.attributes.forEach { attribute ->
                    append(' ').append(escape(attribute.name))
                    attribute.value?.let { append("=\"").append(escape(it)).append("\"") }
                }
            }
            val children = nodesByParent[node.id].orEmpty().sortedWith(compareBy<PreviewNode> { it.order }.thenBy { it.id })
            val markup = if (children.isEmpty()) "&lt;${escape(node.tag)}$attributes&gt;&lt;/${escape(node.tag)}&gt;"
                else "&lt;${escape(node.tag)}$attributes&gt;"
            val badges = buildList {
                when (node.role) {
                    "selected" -> add("SELECTED")
                    "relationalWitness" -> add(":has() witness")
                    "illustrativeSpacer" -> add("illustrative intervening sibling")
                    else -> add("required")
                }
                node.states.forEach { add("required state: :${it.name}") }
            }.joinToString(" &nbsp; ") { "<small><b>${escape(it)}</b></small>" }
            val style = when (node.role) {
                "selected" -> "background-color:#$selectedBackground;border-left:3px solid #$selectedBorder;padding:2px 5px;"
                "relationalWitness" -> "background-color:#$witnessBackground;padding:2px 5px;"
                "illustrativeSpacer" -> "opacity:0.65;padding:2px 5px;"
                else -> "padding:2px 5px;"
            }
            body.append("<div style='$style'><code>$indent$markup</code>&nbsp;&nbsp;$badges</div>")
            children.forEach { appendNode(it, depth + 1) }
            if (children.isNotEmpty()) body.append("<div><code>$indent&lt;/${escape(node.tag)}&gt;</code></div>")
        }
        nodesByParent[null].orEmpty().sortedWith(compareBy<PreviewNode> { it.order }.thenBy { it.id }).forEach { appendNode(it, 0) }
        return "<div class='definition'><b>Selector preview</b><br/><code>${escape(preview.resolvedSelector)}</code></div>" +
            "<div class='content'>$body<hr/><small>Blue: selected element &nbsp; • &nbsp; Shaded: relational witness &nbsp; • &nbsp; " +
            "Representative structure; not project HTML.</small></div>"
    }

    private fun escape(value: String): String = StringUtil.escapeXmlEntities(value)

    companion object {
        private val log = Logger.getInstance(StyleBreezeSelectorDocumentationProvider::class.java)
    }
}
