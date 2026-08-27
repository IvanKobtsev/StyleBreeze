package dev.stylebreeze.gradle;

import java.io.IOException;
import java.io.InputStream;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import java.util.zip.ZipInputStream;
import org.gradle.api.DefaultTask;
import org.gradle.api.GradleException;
import org.gradle.api.file.RegularFileProperty;
import org.gradle.api.tasks.CacheableTask;
import org.gradle.api.tasks.InputFile;
import org.gradle.api.tasks.PathSensitive;
import org.gradle.api.tasks.PathSensitivity;
import org.gradle.api.tasks.TaskAction;

/** Verifies that every supported server executable is present in the packaged plugin. */
@CacheableTask
public abstract class VerifyPluginBinaries extends DefaultTask {
    private static final List<String> EXPECTED = List.of(
            "bin/windows-x64/stylebreeze.exe",
            "bin/windows-arm64/stylebreeze.exe",
            "bin/macos-x64/stylebreeze",
            "bin/macos-arm64/stylebreeze",
            "bin/linux-x64/stylebreeze",
            "bin/linux-arm64/stylebreeze");

    @InputFile
    @PathSensitive(PathSensitivity.RELATIVE)
    public abstract RegularFileProperty getPluginArchive();

    @TaskAction
    public void verify() {
        var archive = getPluginArchive().get().getAsFile();
        var entries = new HashSet<String>();
        try (var zip = new ZipFile(archive)) {
            var outerEntries = zip.entries();
            while (outerEntries.hasMoreElements()) {
                var entry = outerEntries.nextElement();
                if (entry.isDirectory()) {
                    continue;
                }
                entries.add(normalize(entry.getName()));
                if (entry.getName().endsWith(".jar")) {
                    try (var input = zip.getInputStream(entry)) {
                        collectNestedEntries(input, entries);
                    }
                }
            }
        } catch (IOException error) {
            throw new GradleException("Could not inspect plugin archive " + archive, error);
        }

        var missing = EXPECTED.stream()
                .filter(expected -> entries.stream().noneMatch(entry -> matches(entry, expected)))
                .toList();
        if (!missing.isEmpty()) {
            throw new GradleException("Plugin archive is missing executables: " + String.join(", ", missing));
        }
    }

    private static void collectNestedEntries(InputStream input, Set<String> entries) throws IOException {
        try (var nested = new ZipInputStream(input)) {
            ZipEntry entry;
            while ((entry = nested.getNextEntry()) != null) {
                if (!entry.isDirectory()) {
                    entries.add(normalize(entry.getName()));
                }
            }
        }
    }

    private static boolean matches(String entry, String expected) {
        return entry.equals(expected) || entry.endsWith("/" + expected);
    }

    private static String normalize(String path) {
        return path.replace('\\', '/');
    }
}

