package dev.stylebreeze.intellij

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.find.actions.ShowUsagesAction
import com.intellij.find.actions.ShowUsagesActionHandler
import com.intellij.find.actions.ShowUsagesParameters
import com.intellij.internal.statistic.eventLog.events.EventPair
import com.intellij.lang.Language
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.util.SystemInfoRt
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspClient
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiManager
import com.intellij.psi.SmartPointerManager
import com.intellij.psi.SmartPsiElementPointer
import com.intellij.psi.impl.FakePsiElement
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.SearchScope
import com.intellij.ui.awt.RelativePoint
import com.intellij.usageView.UsageInfo
import com.intellij.usages.UsageInfo2UsageAdapter
import com.intellij.usages.UsageSearchPresentation
import com.intellij.usages.UsageSearcher
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
        log.info("StyleBreeze navigation request file=${file.path} offset=$offset clients=${clients.size}")

        for (client in clients) {
            if (isStylesheet(file) && !isCustomPropertyAt(document.charsSequence, offset)) {
                val declaration = request("definition") {
                    client.sendRequestSync(2_000) { server ->
                        server.textDocumentService.definition(
                            DefinitionParams(client.getDocumentIdentifier(file), position),
                        )
                    }
                } ?: continue
                val recognized = !declaration.left.isNullOrEmpty() || !declaration.right.isNullOrEmpty()
                log.info("StyleBreeze definition response file=${file.path} recognized=$recognized left=${declaration.left?.size ?: 0} right=${declaration.right?.size ?: 0}")
                if (!recognized) continue
                val declarationTargets = buildList {
                    declaration.left?.forEach { add(Target(it.uri, it.range)) }
                    declaration.right?.forEach { add(Target(it.targetUri, it.targetSelectionRange)) }
                }
                val selectedDeclaration = declarationTargets.any { target ->
                    client.descriptor.findFileByUri(target.uri)?.identity() == file.identity() &&
                        target.range.contains(position)
                }
                if (!selectedDeclaration) {
                    val mapped = mapTargets(client, declarationTargets)
                    log.info("StyleBreeze forward navigation file=${file.path} protocolTargets=${declarationTargets.size} mappedTargets=${mapped.size}")
                    return mapped.takeIf { it.isNotEmpty() }?.toTypedArray()
                }
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
                } ?: continue
                val targets = mapTargets(
                    client,
                    references.map { Target(it.uri, it.range) },
                ).filterNot { it.containingFile?.virtualFile == file }
                log.info("StyleBreeze reverse navigation file=${file.path} references=${references.size} mappedTargets=${targets.size}")
                return when (targets.size) {
                    // IntelliJ continues to later declaration handlers when an
                    // extension returns an empty array. Return a no-op target so
                    // a recognized module class with no script usages remains
                    // authoritative and built-in CSS providers cannot substitute
                    // unrelated same-named declarations.
                    0 -> arrayOf(StyleBreezeUsagesTarget(element, actualEditor, emptyList()))
                    1 -> targets.toTypedArray()
                    else -> arrayOf(StyleBreezeUsagesTarget(element, actualEditor, targets))
                }
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
            val virtualFile = client.descriptor.findFileByUri(target.uri) ?: run {
                log.warn("StyleBreeze could not map navigation URI ${target.uri}")
                return@mapNotNull null
            }
            val identity = "${virtualFile.identity()}:${target.range.start.line}:${target.range.start.character}"
            if (!seen.add(identity)) return@mapNotNull null
            val psiFile = psiManager.findFile(virtualFile) ?: run {
                log.warn("StyleBreeze could not load PSI for ${virtualFile.path}")
                return@mapNotNull null
            }
            val targetDocument = FileDocumentManager.getInstance().getDocument(virtualFile) ?: run {
                log.warn("StyleBreeze could not load document for ${virtualFile.path}")
                return@mapNotNull null
            }
            val targetOffset = targetDocument.offset(target.range.start) ?: run {
                log.warn("StyleBreeze received invalid target position ${target.range.start} for ${virtualFile.path}")
                return@mapNotNull null
            }
            psiFile.findElementAt(targetOffset) ?: psiFile
        }
    }

    companion object {
        private val log = Logger.getInstance(StyleBreezeGotoDeclarationHandler::class.java)

        private fun isStylesheet(file: VirtualFile): Boolean {
            val name = file.name.lowercase()
            return name.endsWith(".module.css") || name.endsWith(".scss")
        }

        private fun isCustomPropertyAt(text: CharSequence, offset: Int): Boolean {
            var start = offset.coerceAtMost(text.length)
            while (start > 0 && (text[start - 1].isLetterOrDigit() || text[start - 1] == '_' || text[start - 1] == '-')) start--
            return start + 1 < text.length && text[start] == '-' && text[start + 1] == '-'
        }
    }
}

private class StyleBreezeUsagesTarget(
    declaration: PsiElement,
    private val editor: Editor,
    usages: List<PsiElement>,
) : FakePsiElement() {
    private val declarationPointer = SmartPointerManager.createPointer(declaration)
    private val usagePointers = usages.map(SmartPointerManager::createPointer)

    override fun getParent(): PsiElement? = declarationPointer.element

    override fun getName(): String? = declarationPointer.element?.text

    override fun canNavigate(): Boolean = true

    override fun canNavigateToSource(): Boolean = true

    override fun navigate(requestFocus: Boolean) {
        val declaration = declarationPointer.element ?: return
        val validUsages = usagePointers.mapNotNull(SmartPsiElementPointer<PsiElement>::getElement)
        if (validUsages.isEmpty()) return
        if (validUsages.size == 1) {
            val usage = validUsages.single()
            val usageFile = usage.containingFile?.virtualFile ?: return
            OpenFileDescriptor(usage.project, usageFile, usage.textOffset).navigate(requestFocus)
            return
        }
        val handler = StyleBreezeShowUsagesHandler(declaration, validUsages)
        val parameters = ShowUsagesParameters.initial(
            declaration.project,
            editor,
            RelativePoint.getCenterOf(editor.contentComponent),
        )
        ShowUsagesAction.showElementUsagesWithResult(
            parameters,
            handler,
            handler.createUsageView(declaration.project),
        )
    }
}

private class StyleBreezeShowUsagesHandler(
    private val declaration: PsiElement,
    usageElements: List<PsiElement>,
) : ShowUsagesActionHandler {
    private val usages = usageElements.map { UsageInfo2UsageAdapter(UsageInfo(it)) }
    private val scope = GlobalSearchScope.projectScope(declaration.project)

    override fun isValid(): Boolean = declaration.isValid

    override fun getPresentation(): UsageSearchPresentation = object : UsageSearchPresentation {
        override fun getSearchTargetString(): String = declaration.text

        override fun getOptionsString(): String = "StyleBreeze CSS Module usages"
    }

    override fun createUsageSearcher(): UsageSearcher = UsageSearcher { processor ->
        usages.forEach { if (!processor.process(it)) return@UsageSearcher }
    }

    override fun findUsages() = Unit

    override fun showDialog(): ShowUsagesActionHandler = this

    override fun withScope(scope: SearchScope): ShowUsagesActionHandler = this

    override fun moreUsages(parameters: ShowUsagesParameters): ShowUsagesParameters = parameters.moreUsages()

    override fun getSelectedScope(): SearchScope = scope

    override fun getMaximalScope(): SearchScope = scope

    override fun getTargetLanguage(): Language = declaration.language

    override fun getTargetClass(): Class<*> = declaration.javaClass

    override fun getEventData(): List<EventPair<*>> = mutableListOf()

    override fun navigateToSingleUsageImmediately(): Boolean = false

    override fun buildFinishEventData(usage: UsageInfo?): List<EventPair<*>> = mutableListOf()
}

private data class Target(val uri: String, val range: org.eclipse.lsp4j.Range)

private fun org.eclipse.lsp4j.Range.contains(position: Position): Boolean =
    (position.line > start.line || position.line == start.line && position.character >= start.character) &&
        (position.line < end.line || position.line == end.line && position.character <= end.character)

private fun VirtualFile.identity(): String = if (SystemInfoRt.isWindows) path.lowercase() else path

private fun com.intellij.openapi.editor.Document.offset(position: Position): Int? {
    if (position.line !in 0 until lineCount) return null
    val start = getLineStartOffset(position.line)
    val end = getLineEndOffset(position.line)
    return (start + position.character).coerceIn(start, end)
}
