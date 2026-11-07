'use strict';

const vscode = require('vscode');
const { realpath, lstat } = require('node:fs/promises');
const path = require('node:path');
const {
  ClientError,
  identifyExecutable,
  isLocalPath,
  isToken,
  makeClient,
  sameOrInside,
  storedConnection,
} = require('./client.cjs');

const connectionKey = 'voldiq.connection';
let active;

function localUri(uri) {
  return uri?.scheme === 'file' && !uri.authority && isLocalPath(uri.fsPath);
}

function requireLocalWorkspace() {
  if (process.platform !== 'win32' || vscode.env.remoteName || !vscode.workspace.isTrusted) {
    throw new ClientError('scope', 'Voldiq requires a trusted local Windows workspace.');
  }
  if ((vscode.workspace.workspaceFolders ?? []).some(({ uri }) => !localUri(uri))) {
    throw new ClientError('scope', 'Voldiq supports local Windows folders only.');
  }
}

async function checkExecutableLocation(executable) {
  requireLocalWorkspace();
  for (const { uri } of vscode.workspace.workspaceFolders ?? []) {
    const folder = await realpath(uri.fsPath);
    if (sameOrInside(folder, executable)) {
      throw new ClientError(
        'executable',
        'Choose a trusted Voldiq CLI outside the open workspace.',
      );
    }
  }
}

function safeLine(value) {
  return value.replace(
    /[\u0000-\u001f\u007f-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/gu,
    '\ufffd',
  );
}

function count(value) {
  return Number.isSafeInteger(value) && value >= 0 ? value.toLocaleString() : 'Unreported';
}

function bytes(value) {
  if (!Number.isSafeInteger(value) || value < 0) return 'Unreported';
  if (value < 1024) return `${count(value)} B`;
  const unit = Math.min(5, Math.floor(Math.log2(value) / 10));
  return `${(value / 1024 ** unit).toLocaleString(undefined, { maximumFractionDigits: 1 })} ${['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'][unit]} (${count(value)} B)`;
}

function showReport(output, result) {
  const { stats, items, job, snapshot } = result;
  output.clear();
  output.appendLine(`Folder: ${safeLine(stats.root)}`);
  output.appendLine(
    `Snapshot ${snapshot} · Job ${job} · ${stats.incomplete ? 'Partial coverage' : 'Complete for this scan scope'}`,
  );
  output.appendLine(`Logical size: ${bytes(stats.logical)}`);
  output.appendLine(`Allocated size: ${bytes(stats.allocated)}`);
  output.appendLine(
    `Files: ${count(stats.files)} · Folders: ${count(stats.folders)} · Reported errors: ${count(stats.errors)}`,
  );
  output.appendLine('');
  output.appendLine(
    items.length ? `Largest files shown (${items.length})` : 'No files returned for this scope.',
  );
  for (const item of items) output.appendLine(`  ${bytes(item.logical)} · ${safeLine(item.name)}`);
  output.show();
}

async function chooseFolder(resource) {
  if (resource) {
    if (!localUri(resource)) throw new ClientError('scope', 'Choose a local Windows folder.');
  } else {
    [resource] =
      (await vscode.window.showOpenDialog({
        title: 'Choose a folder to analyze with Voldiq',
        canSelectFolders: true,
        canSelectFiles: false,
        canSelectMany: false,
        defaultUri: vscode.workspace.workspaceFolders?.[0]?.uri,
        openLabel: 'Analyze folder',
      })) ?? [];
    if (!resource) return null;
  }
  if (!localUri(resource)) throw new ClientError('scope', 'Choose a local Windows folder.');
  const metadata = await vscode.workspace.fs.stat(resource);
  if (
    !(metadata.type & vscode.FileType.Directory) ||
    metadata.type & vscode.FileType.SymbolicLink
  ) {
    throw new ClientError('scope', 'Choose a regular local folder.');
  }
  return resource.fsPath;
}

function activate(context) {
  const output = vscode.window.createOutputChannel('Voldiq');
  let configuring = false;
  const command = (id, action) =>
    vscode.commands.registerCommand(id, async (...args) => {
      try {
        requireLocalWorkspace();
        if (active || configuring)
          throw new ClientError('busy', 'Wait for the current Voldiq action to finish.');
        await action(...args);
      } catch (error) {
        if (error instanceof ClientError && error.code === 'cancelled') {
          void vscode.window.showInformationMessage('Voldiq scan cancelled.');
        } else {
          void vscode.window.showErrorMessage(
            error instanceof ClientError ? error.message : 'Voldiq could not complete this action.',
          );
        }
      }
    });

  context.subscriptions.push(
    output,
    command('voldiq.connect', async () => {
      configuring = true;
      try {
        const selected = await vscode.window.showOpenDialog({
          title: 'Choose the trusted Voldiq CLI',
          openLabel: 'Choose voldiq-cli.exe',
          filters: { 'Windows executables': ['exe'] },
        });
        if (!selected?.length) return;
        if (!localUri(selected[0]))
          throw new ClientError('executable', 'Choose a local voldiq-cli.exe file.');
        const identity = await identifyExecutable(selected[0].fsPath);
        await checkExecutableLocation(identity.executable);
        const state = await vscode.window.showQuickPick(
          ['Default desktop state', 'Choose a portable state folder'],
          { title: 'Voldiq state location' },
        );
        if (!state) return;
        let stateDirectory = null;
        if (state === 'Choose a portable state folder') {
          const portable = await vscode.window.showOpenDialog({
            title: 'Choose the existing Voldiq state folder',
            canSelectFiles: false,
            canSelectFolders: true,
          });
          if (!portable?.length) return;
          if (!localUri(portable[0]))
            throw new ClientError('scope', 'Choose an existing local Voldiq state folder.');
          stateDirectory = await realpath(portable[0].fsPath);
          if (!(await lstat(path.join(stateDirectory, 'agents.db'))).isFile()) {
            throw new ClientError(
              'scope',
              'The selected folder does not contain a Voldiq connection registry.',
            );
          }
        }
        const input = await vscode.window.showInputBox({
          title: 'Voldiq connection credential',
          prompt: 'Paste the credential from Voldiq Settings → Agents and permissions.',
          password: true,
          ignoreFocusOut: true,
          validateInput: (value) =>
            isToken(value.trim()) ? undefined : 'Enter the 64-character credential from Voldiq.',
        });
        if (!input) return;
        const connection = { ...identity, stateDirectory, token: input.trim() };
        await makeClient(connection, checkExecutableLocation).check();
        await context.secrets.store(connectionKey, JSON.stringify(connection));
        void vscode.window.showInformationMessage('Voldiq scan connection saved.');
      } finally {
        configuring = false;
      }
    }),
    command('voldiq.disconnect', async () => {
      await context.secrets.delete(connectionKey);
      output.clear();
      void vscode.window.showInformationMessage('Voldiq connection removed from VS Code.');
    }),
    command('voldiq.analyzeFolder', async (resource) => {
      const controller = new AbortController();
      active = controller;
      try {
        const connection = storedConnection(await context.secrets.get(connectionKey));
        if (!connection)
          throw new ClientError('authorization', 'Run Voldiq: Connect to Voldiq first.');
        const folder = await chooseFolder(resource);
        if (!folder) return;
        const client = makeClient(connection, checkExecutableLocation);
        const result = await vscode.window.withProgress(
          {
            title: 'Voldiq folder analysis',
            location: vscode.ProgressLocation.Notification,
            cancellable: true,
          },
          async (progress, cancellation) => {
            const subscription = cancellation.onCancellationRequested(() => controller.abort());
            if (cancellation.isCancellationRequested) controller.abort();
            try {
              return await client.analyze(folder, controller.signal, (message) =>
                progress.report({ message }),
              );
            } finally {
              subscription.dispose();
            }
          },
        );
        if (result.kind === 'completed_while_cancelling') {
          void vscode.window.showInformationMessage(
            `The Voldiq scan completed as cancellation was requested. Snapshot ${result.snapshot} is available in Voldiq.`,
          );
        } else showReport(output, result);
      } finally {
        active = undefined;
      }
    }),
  );
}

function deactivate() {
  active?.abort();
}

module.exports = { activate, deactivate };
