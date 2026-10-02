import { chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const plugin = join(root, 'editors', 'jetbrains');
const nativeRoot = join(root, 'dist', 'jetbrains');
const output = join(root, 'dist', 'editors');
const targets = [
  ['win32-x64', 'style-breeze.exe'], ['win32-arm64', 'style-breeze.exe'],
  ['darwin-x64', 'style-breeze'], ['darwin-arm64', 'style-breeze'],
  ['linux-x64', 'style-breeze'], ['linux-arm64', 'style-breeze'],
];

for (const [target, name] of targets) requireFile(join(nativeRoot, target, name));
const gradle = process.platform === 'win32' ? 'gradle.bat' : 'gradle';
run(gradle, ['buildPlugin', 'test', 'verifyPluginStructure', 'verifyBundledBinaries', '--rerun-tasks', '--no-configuration-cache'], plugin);
const version = Object.fromEntries(readFileSync(join(plugin, 'gradle.properties'), 'utf8').split(/\r?\n/).filter(Boolean).map(line => line.split('=', 2))).version;
const built = join(plugin, 'build', 'distributions', `style-breeze-jetbrains-${version}.zip`);
requireFile(built);
mkdirSync(output, { recursive: true });
const artifact = join(output, `style-breeze-webstorm-${version}.zip`);
rmSync(artifact, { force: true });
copyFileSync(built, artifact);
console.log(`Created ${artifact}`);

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, stdio: 'inherit', shell: process.platform === 'win32' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
}
function requireFile(path) {
  if (!existsSync(path) || !statSync(path).isFile() || statSync(path).size === 0) throw new Error(`Missing file: ${path}`);
}
