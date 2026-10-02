package dev.stylebreeze.jetbrains

import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.components.StoragePathMacros
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import java.nio.file.Path

@Service(Service.Level.PROJECT)
@State(name = "StyleBreezeSettings", storages = [Storage(StoragePathMacros.WORKSPACE_FILE)])
class StyleBreezeSettings : PersistentStateComponent<StyleBreezeSettings.Data> {
    data class Data(
        var initialized: Boolean = false,
        var userConfigured: Boolean = false,
        var showDiagnostics: Boolean = false,
        var configLocation: String = ConfigLocation.WORKSPACE_ROOT.name,
        var configPath: String = "",
    )

    enum class ConfigLocation { WORKSPACE_ROOT, CUSTOM_PATH }
    private var data = Data()
    override fun getState(): Data = data
    override fun loadState(state: Data) { data = state }

    fun location(): ConfigLocation = runCatching { ConfigLocation.valueOf(data.configLocation) }
        .getOrDefault(ConfigLocation.WORKSPACE_ROOT)

    fun initialize(project: Project) {
        if (data.initialized) return
        data.showDiagnostics = project.basePath?.let { Path.of(it, CONFIG_NAME).toFile().isFile } == true
        data.initialized = true
    }

    fun configPath(project: Project): Path? {
        val root = project.basePath?.let(Path::of) ?: return null
        return when (location()) {
            ConfigLocation.WORKSPACE_ROOT -> root.resolve(CONFIG_NAME)
            ConfigLocation.CUSTOM_PATH -> data.configPath.trim().takeIf(String::isNotEmpty)?.let {
                Path.of(it).let { value -> if (value.isAbsolute) value else root.resolve(value) }
            }
        }?.normalize()
    }

    companion object {
        const val CONFIG_NAME = "style-contract.json"
        fun getInstance(project: Project): StyleBreezeSettings = project.service()
    }
}
