package dev.stylebreeze.jetbrains

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.find.actions.ShowUsagesAction
import com.intellij.find.actions.ShowUsagesActionHandler
import com.intellij.find.actions.ShowUsagesParameters
import com.intellij.internal.statistic.eventLog.events.EventPair
import com.intellij.lang.Language
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
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
import org.eclipse.lsp4j.Location
import org.eclipse.lsp4j.Position
import org.eclipse.lsp4j.DefinitionParams
import org.eclipse.lsp4j.ReferenceContext
import org.eclipse.lsp4j.ReferenceParams
import org.eclipse.lsp4j.jsonrpc.messages.Either
import java.net.URI
import java.util.concurrent.Callable
import java.util.concurrent.TimeUnit

/**
 * Gives StyleBreeze's module-aware navigation precedence over WebStorm's
 * name-based CSS navigation. The platform LSP bridge is only an implicit
 * reference provider, so it loses whenever JavaScript or CSS supplies a
 * native reference of its own.
 */
class StyleBreezeGotoDeclarationHandler : GotoDeclarationHandler {
    override fun getGotoDeclarationTargets(
        sourceElement: PsiElement?,
        offset: Int,
        editor: Editor?,
    ): Array<PsiElement>? {
        val element = sourceElement ?: return null
        val actualEditor = editor ?: return null
        val sourceFile = element.containingFile?.virtualFile ?: return null
        val project = element.project
        if (!isEnabledFor(project, sourceFile.extension)) return null

        val document = actualEditor.document
        val position = document.positionAt(offset) ?: return null
        val server = LspClientManager.getInstance(project)
            .getClients(StyleBreezeLspIntegrationProvider::class.java)
            .firstOrNull { it.descriptor.isSupportedFile(sourceFile) }
            ?: return null
        val identifier = server.getDocumentIdentifier(sourceFile)

        val response = requestFromServer("definition") {
            server.sendRequestSync(2_000) {
                it.textDocumentService.definition(DefinitionParams(identifier, position))
            }
        } ?: return null
        val locations = response.locations()
        if (locations.isNotEmpty()) {
            val targets = locations.toPsiElements(project)
            return when (targets.size) {
                0 -> null
                1 -> targets
                else -> arrayOf(StyleBreezeUsagesTarget(element, actualEditor, targets.toList()))
            }
        }

        // An empty array is authoritative to the declaration action and stops
        // WebStorm from falling back to unrelated same-named CSS classes. Ask
        // for declarations too so a known declaration with zero usages can be
        // distinguished from an unrelated JavaScript/CSS symbol.
        val recognized = requestFromServer("reference resolution") {
            server.sendRequestSync(2_000) {
                it.textDocumentService.references(
                    ReferenceParams(identifier, position, ReferenceContext(true)),
                )
            }
        }?.isNotEmpty() == true
        return if (recognized) {
            arrayOf(StyleBreezeUsagesTarget(element, actualEditor, emptyList()))
        } else {
            null
        }
    }

    private fun isEnabledFor(project: Project, extension: String?): Boolean {
        val settings = StyleBreezeSettings.getInstance(project)
        settings.initialize(project)
        return settings.state.showDiagnostics &&
            settings.configPath(project)?.toFile()?.isFile == true &&
            extension?.lowercase() in supportedExtensions
    }

    private fun Either<List<Location>, List<org.eclipse.lsp4j.LocationLink>>.locations(): List<Location> =
        if (isLeft) left.orEmpty() else right.orEmpty().map { Location(it.targetUri, it.targetSelectionRange) }

    private fun <T> requestFromServer(operation: String, request: () -> T): T? =
        runCatching {
            AppExecutorUtil.getAppExecutorService()
                .submit(Callable(request))
                .get(2_500, TimeUnit.MILLISECONDS)
        }.onFailure {
            log.warn("StyleBreeze $operation navigation request failed", it)
        }.getOrNull()

    private fun List<Location>.toPsiElements(project: Project): Array<PsiElement> =
        mapNotNull { location ->
            val path = runCatching { URI(location.uri) }.getOrNull()?.takeIf { it.scheme == "file" }?.let(java.nio.file.Path::of)
                ?: return@mapNotNull null
            val file = LocalFileSystem.getInstance().refreshAndFindFileByNioFile(path) ?: return@mapNotNull null
            val psiFile = PsiManager.getInstance(project).findFile(file) ?: return@mapNotNull null
            val document = FileDocumentManager.getInstance().getDocument(file) ?: return@mapNotNull null
            val targetOffset = document.offsetAt(location.range.start) ?: return@mapNotNull null
            psiFile.findElementAt(targetOffset)
        }.distinct().toTypedArray()

    private fun com.intellij.openapi.editor.Document.positionAt(offset: Int): Position? {
        if (offset !in 0..textLength) return null
        val line = getLineNumber(offset)
        return Position(line, offset - getLineStartOffset(line))
    }

    private fun com.intellij.openapi.editor.Document.offsetAt(position: Position): Int? {
        if (position.line !in 0 until lineCount) return null
        val start = getLineStartOffset(position.line)
        val end = getLineEndOffset(position.line)
        return (start + position.character).takeIf { it in start..end }
    }

    private companion object {
        val log = Logger.getInstance(StyleBreezeGotoDeclarationHandler::class.java)
        val supportedExtensions = setOf("ts", "tsx", "css", "scss")
    }
}

private class StyleBreezeUsagesTarget(
    source: PsiElement,
    private val editor: Editor,
    targets: List<PsiElement>,
) : FakePsiElement() {
    private val sourcePointer = SmartPointerManager.createPointer(source)
    private val targetPointers = targets.map(SmartPointerManager::createPointer)

    override fun getParent(): PsiElement? = sourcePointer.element
    override fun getName(): String? = sourcePointer.element?.text
    override fun canNavigate(): Boolean = true
    override fun canNavigateToSource(): Boolean = true

    override fun navigate(requestFocus: Boolean) {
        val source = sourcePointer.element ?: return
        val targets = targetPointers.mapNotNull(SmartPsiElementPointer<PsiElement>::getElement)
        if (targets.size == 1) {
            val target = targets.single()
            val file = target.containingFile?.virtualFile ?: return
            OpenFileDescriptor(target.project, file, target.textOffset).navigate(requestFocus)
            return
        }

        val handler = StyleBreezeShowUsagesHandler(source, targets)
        val parameters = ShowUsagesParameters.initial(
            source.project,
            editor,
            RelativePoint.getCenterOf(editor.contentComponent),
        )
        ShowUsagesAction.showElementUsagesWithResult(
            parameters,
            handler,
            handler.createUsageView(source.project),
        )
    }
}

private class StyleBreezeShowUsagesHandler(
    private val source: PsiElement,
    targetElements: List<PsiElement>,
) : ShowUsagesActionHandler {
    private val usages = targetElements.map { UsageInfo2UsageAdapter(UsageInfo(it)) }
    private val scope = GlobalSearchScope.projectScope(source.project)

    override fun isValid(): Boolean = source.isValid
    override fun getPresentation(): UsageSearchPresentation = object : UsageSearchPresentation {
        override fun getSearchTargetString(): String = source.text
        override fun getOptionsString(): String = "StyleBreeze navigation"
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
    override fun getTargetLanguage(): Language = source.language
    override fun getTargetClass(): Class<*> = source.javaClass
    override fun getEventData(): List<EventPair<*>> = mutableListOf()
    override fun navigateToSingleUsageImmediately(): Boolean = false
    override fun buildFinishEventData(usage: UsageInfo?): List<EventPair<*>> = mutableListOf()
}
