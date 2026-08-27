import * as fs from 'node:fs';
import * as path from 'node:path';
import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Trace,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const settings = vscode.workspace.getConfiguration('styleBreeze');
  const configuredServer = settings.get<string>('server.path', '').trim();
  const executable = configuredServer || bundledServer(context);

  if (!fs.existsSync(executable)) {
    void vscode.window.showErrorMessage(
      `StyleBreeze server was not found at ${executable}. Configure styleBreeze.server.path.`,
    );
    return;
  }
  if (process.platform !== 'win32') fs.chmodSync(executable, 0o755);

  const serverOptions: ServerOptions = {
    command: executable,
    args: ['--stdio'],
    options: { cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath },
  };
  const stylesheetWatcher = vscode.workspace.createFileSystemWatcher(
    '**/*.module.{css,scss}',
  );
  const scriptWatcher = vscode.workspace.createFileSystemWatcher(
    '**/*.{js,jsx,ts,tsx}',
  );
  context.subscriptions.push(stylesheetWatcher, scriptWatcher);

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', language: 'css', pattern: '**/*.module.css' },
      { scheme: 'file', language: 'scss', pattern: '**/*.module.scss' },
      { scheme: 'file', language: 'typescript' },
      { scheme: 'file', language: 'typescriptreact' },
      { scheme: 'file', language: 'javascript' },
      { scheme: 'file', language: 'javascriptreact' },
    ],
    synchronize: {
      configurationSection: 'styleBreeze',
      fileEvents: [stylesheetWatcher, scriptWatcher],
    },
    middleware: {
      provideDefinition: async (document, position, token, next) => {
        if (!isModuleStylesheet(document)) {
          return next(document, position, token);
        }
        const references = await client?.sendRequest<ProtocolLocation[]>(
          'textDocument/references',
          {
            textDocument: { uri: document.uri.toString() },
            position,
            context: { includeDeclaration: false },
          },
          token,
        );
        return references?.map(
          (location) =>
            new vscode.Location(
              vscode.Uri.parse(location.uri),
              new vscode.Range(
                new vscode.Position(location.range.start.line, location.range.start.character),
                new vscode.Position(location.range.end.line, location.range.end.character),
              ),
            ),
        ) ?? [];
      },
    },
    outputChannelName: 'StyleBreeze',
  };

  client = new LanguageClient('styleBreeze', 'StyleBreeze', serverOptions, clientOptions);
  const trace = settings.get<string>('server.trace', 'off');
  client.setTrace(
    trace === 'verbose' ? Trace.Verbose : trace === 'messages' ? Trace.Messages : Trace.Off,
  );
  await client.start();
  context.subscriptions.push({ dispose: () => void client?.stop() });
}

export async function deactivate(): Promise<void> {
  await client?.stop();
}

function bundledServer(context: vscode.ExtensionContext): string {
  const platform = `${process.platform}-${process.arch}`;
  const name = process.platform === 'win32' ? 'stylebreeze.exe' : 'stylebreeze';
  return context.asAbsolutePath(path.join('bin', platform, name));
}

function isModuleStylesheet(document: vscode.TextDocument): boolean {
  const name = document.uri.path.toLowerCase();
  return name.endsWith('.module.css') || name.endsWith('.module.scss');
}

interface ProtocolLocation {
  uri: string;
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
}
