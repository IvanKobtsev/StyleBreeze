package dev.stylebreeze.jetbrains

import com.intellij.openapi.fileChooser.FileChooserDescriptor
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.options.ConfigurationException
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.ui.TitledSeparator
import com.intellij.util.ui.JBUI
import java.awt.GridBagConstraints
import java.awt.GridBagLayout
import javax.swing.*

class StyleBreezeConfigurable(private val project: Project) : Configurable {
    private val enabled = JCheckBox("Show diagnostics")
    private val section = TitledSeparator("StyleContract config path")
    private val root = JRadioButton("Workspace root")
    private val custom = JRadioButton("Custom path")
    private val path = TextFieldWithBrowseButton()
    private var panel: JPanel? = null

    override fun getDisplayName() = "StyleBreeze"
    override fun createComponent(): JComponent {
        ButtonGroup().apply { add(root); add(custom) }
        enabled.addActionListener { updateEnabled() }
        root.addActionListener { updateEnabled() }
        custom.addActionListener { updateEnabled() }
        path.addBrowseFolderListener(project, FileChooserDescriptor(true, false, false, false, false, false)
            .withFileFilter { it.extension.equals("json", true) || it.extension.equals("json5", true) })
        val constraints = GridBagConstraints().apply {
            anchor = GridBagConstraints.WEST; fill = GridBagConstraints.HORIZONTAL; insets = JBUI.insets(4)
        }
        return JPanel(GridBagLayout()).also { result ->
            constraints.gridx = 0; constraints.gridy = 0; constraints.gridwidth = 2; result.add(enabled, constraints)
            constraints.gridy = 1; result.add(section, constraints)
            constraints.gridy = 2; result.add(root, constraints)
            constraints.gridy = 3; result.add(custom, constraints)
            constraints.gridy = 4; constraints.gridwidth = 1; result.add(JLabel("Path:"), constraints)
            constraints.gridx = 1; constraints.weightx = 1.0; result.add(path, constraints)
            constraints.gridx = 0; constraints.gridy = 5; constraints.gridwidth = 2; constraints.weighty = 1.0
            constraints.fill = GridBagConstraints.BOTH; result.add(JPanel(), constraints)
            panel = result
            reset()
        }
    }

    override fun isModified(): Boolean {
        val state = StyleBreezeSettings.getInstance(project).state
        return enabled.isSelected != state.showDiagnostics ||
            selected().name != state.configLocation || path.text.trim() != state.configPath
    }

    override fun apply() {
        if (enabled.isSelected && custom.isSelected && path.text.isBlank()) {
            throw ConfigurationException("Choose a StyleContract configuration file for Custom path.")
        }
        val state = StyleBreezeSettings.getInstance(project).state
        state.initialized = true
        state.userConfigured = true
        state.showDiagnostics = enabled.isSelected
        state.configLocation = selected().name
        state.configPath = path.text.trim()
        project.getService(StyleBreezeIssueNotifier::class.java).checkConfiguration()
        LspClientManager.getInstance(project).stopAndRestartClientsIfNeeded(StyleBreezeLspIntegrationProvider::class.java)
    }

    override fun reset() {
        val settings = StyleBreezeSettings.getInstance(project)
        settings.initialize(project)
        enabled.isSelected = settings.state.showDiagnostics
        root.isSelected = settings.location() == StyleBreezeSettings.ConfigLocation.WORKSPACE_ROOT
        custom.isSelected = !root.isSelected
        path.text = settings.state.configPath
        updateEnabled()
    }

    override fun disposeUIResources() { panel = null }
    private fun selected() = if (custom.isSelected) StyleBreezeSettings.ConfigLocation.CUSTOM_PATH else StyleBreezeSettings.ConfigLocation.WORKSPACE_ROOT
    private fun updateEnabled() {
        section.isEnabled = enabled.isSelected
        root.isEnabled = enabled.isSelected
        custom.isEnabled = enabled.isSelected
        path.isEnabled = enabled.isSelected && custom.isSelected
        panel?.revalidate(); panel?.repaint()
    }
}
