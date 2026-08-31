package dev.stylebreeze.intellij

import org.eclipse.lsp4j.Range
import org.eclipse.lsp4j.TextDocumentPositionParams
import org.eclipse.lsp4j.jsonrpc.services.JsonRequest
import org.eclipse.lsp4j.services.LanguageServer
import java.util.concurrent.CompletableFuture

interface StyleBreezeLanguageServer : LanguageServer {
    @JsonRequest("stylebreeze/selectorPreview")
    fun selectorPreview(params: TextDocumentPositionParams): CompletableFuture<SelectorPreviewResponse?>
}

data class SelectorPreviewResponse(
    val range: Range? = null,
    val preview: SelectorPreview? = null,
    val unsupported: UnsupportedReason? = null,
)

data class UnsupportedReason(val message: String = "Unsupported selector")
data class SelectorPreview(
    val resolvedSelector: String = "",
    val nodes: List<PreviewNode> = emptyList(),
    val relationships: List<Relationship> = emptyList(),
    val subject: Int = 0,
)
data class PreviewNode(
    val id: Int = 0,
    val tag: String = "div",
    val elementId: String? = null,
    val classes: List<String> = emptyList(),
    val attributes: List<PreviewAttribute> = emptyList(),
    val states: List<StateRequirement> = emptyList(),
    val role: String = "requiredSupport",
    val parent: Int? = null,
    val order: Int = 0,
)
data class PreviewAttribute(val name: String = "", val value: String? = null)
data class StateRequirement(val name: String = "")
data class Relationship(val from: Int = 0, val to: Int = 0, val kind: String = "descendant")
