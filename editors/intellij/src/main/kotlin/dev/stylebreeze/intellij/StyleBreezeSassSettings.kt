package dev.stylebreeze.intellij

import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.options.SearchableConfigurable
import com.intellij.openapi.project.Project
import com.intellij.platform.lsp.api.LspClientManager
import org.eclipse.lsp4j.DidChangeConfigurationParams
import java.awt.BorderLayout
import javax.swing.JCheckBox
import javax.swing.JComponent
import javax.swing.JLabel
import javax.swing.JPanel
import javax.swing.JScrollPane
import javax.swing.JTextArea
import java.util.concurrent.CompletableFuture
import java.util.WeakHashMap

@Service(Service.Level.PROJECT)
@State(name = "StyleBreezeSassSettings", storages = [Storage("styleBreeze.xml")])
class StyleBreezeSassSettings : PersistentStateComponent<StyleBreezeSassSettings.State> {
    data class State(
        var loadPaths: MutableList<String> = mutableListOf("."),
        var fixImportsOnSave: Boolean = true,
    )

    private var value = State()
    override fun getState(): State = value
    override fun loadState(state: State) { value = state }
}

class StyleBreezeSassConfigurable(private val project: Project) : SearchableConfigurable {
    private val roots = JTextArea(6, 45)
    private val fixOnSave = JCheckBox("Fix relative @use and @forward paths on save")
    private var panel: JPanel? = null

    override fun getId(): String = "stylebreeze.scss"
    override fun getDisplayName(): String = "StyleBreeze SCSS"
    override fun createComponent(): JComponent = JPanel(BorderLayout(0, 8)).also {
        val header = JPanel(BorderLayout()).apply {
            add(JLabel("Sass load paths (one project-relative or absolute path per line):"), BorderLayout.NORTH)
            add(JScrollPane(roots), BorderLayout.CENTER)
        }
        it.add(header, BorderLayout.CENTER)
        it.add(fixOnSave, BorderLayout.SOUTH)
        panel = it
        reset()
    }
    override fun isModified(): Boolean {
        val state = project.getService(StyleBreezeSassSettings::class.java).state
        return paths() != state.loadPaths || fixOnSave.isSelected != state.fixImportsOnSave
    }
    override fun apply() {
        val state = project.getService(StyleBreezeSassSettings::class.java).state
        state.loadPaths = paths().toMutableList()
        state.fixImportsOnSave = fixOnSave.isSelected
        publishSassSettings(project)
    }
    override fun reset() {
        val state = project.getService(StyleBreezeSassSettings::class.java).state
        roots.text = state.loadPaths.joinToString("\n")
        fixOnSave.isSelected = state.fixImportsOnSave
    }
    override fun disposeUIResources() { panel = null }
    private fun paths(): List<String> = roots.text.lineSequence().map(String::trim).filter(String::isNotEmpty).toList().ifEmpty { listOf(".") }
}

internal fun publishSassSettings(project: Project) {
    val state = project.getService(StyleBreezeSassSettings::class.java).state
    val snapshot = state.loadPaths.toList()
    val clients = LspClientManager.getInstance(project).getClients(StyleBreezeLspProvider::class.java)
    if (clients.isEmpty()) return
    synchronized(published) {
        if (published[project] == snapshot) return
    }
    val settings = mapOf("styleBreeze" to mapOf("scss" to mapOf("loadPaths" to state.loadPaths)))
    val sent = clients.all { client ->
        runCatching {
        client.sendRequestSync(2_000) { server ->
            server.workspaceService.didChangeConfiguration(DidChangeConfigurationParams(settings))
            CompletableFuture.completedFuture(Unit)
        }
        }.isSuccess
    }
    if (sent) synchronized(published) { published[project] = snapshot }
}

private val published = WeakHashMap<Project, List<String>>()
