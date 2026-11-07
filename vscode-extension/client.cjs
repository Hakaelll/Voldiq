'use strict';

const { spawn } = require('node:child_process');
const { createHash } = require('node:crypto');
const { open, realpath } = require('node:fs/promises');
const path = require('node:path');

const maximumOutput = 256 * 1024;
const maximumProgressLine = 4096;
const maximumExecutable = 64 * 1024 * 1024;
const pageSize = 20;

class ClientError extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

function incompatible() {
  return new ClientError(
    'protocol',
    'Voldiq returned an unsupported or incomplete reply. Check the CLI version.',
  );
}

function isId(value) {
  return Number.isSafeInteger(value) && value > 0;
}

function isToken(value) {
  return typeof value === 'string' && /^[\da-f]{64}$/i.test(value);
}

function isLocalPath(value) {
  return typeof value === 'string' && /^[a-z]:[\\/]/i.test(value) && !value.includes('\0');
}

function sameOrInside(folder, target) {
  const relative = path.win32.relative(folder, target);
  return (
    relative === '' ||
    (!path.win32.isAbsolute(relative) && relative !== '..' && !relative.startsWith('..\\'))
  );
}

async function identifyExecutable(candidate) {
  if (
    !isLocalPath(candidate) ||
    path.win32.basename(candidate).toLowerCase() !== 'voldiq-cli.exe'
  ) {
    throw new ClientError('executable', 'Choose a local voldiq-cli.exe file.');
  }
  const executable = await realpath(candidate);
  if (
    !isLocalPath(executable) ||
    path.win32.basename(executable).toLowerCase() !== 'voldiq-cli.exe'
  ) {
    throw new ClientError('executable', 'Choose a local voldiq-cli.exe file.');
  }
  const file = await open(executable, 'r');
  try {
    const before = await file.stat({ bigint: true });
    if (!before.isFile() || before.size < 2n || before.size > BigInt(maximumExecutable)) {
      throw new ClientError('executable', 'The selected file is outside the supported CLI size.');
    }
    const signature = Buffer.alloc(2);
    const { bytesRead } = await file.read(signature, 0, signature.length, 0);
    if (bytesRead !== 2 || signature.toString('ascii') !== 'MZ') {
      throw new ClientError('executable', 'The selected file is not a Windows executable.');
    }
    const hash = createHash('sha256');
    for await (const chunk of file.createReadStream({
      autoClose: false,
      start: 0,
      end: Number(before.size) - 1,
    })) {
      hash.update(chunk);
    }
    const after = await file.stat({ bigint: true });
    if (
      before.size !== after.size ||
      before.ctimeNs !== after.ctimeNs ||
      before.mtimeNs !== after.mtimeNs
    ) {
      throw new ClientError('executable', 'The CLI changed while it was being checked. Try again.');
    }
    return { executable, sha256: hash.digest('hex') };
  } finally {
    await file.close();
  }
}

function storedConnection(raw) {
  if (typeof raw !== 'string') return null;
  try {
    const data = JSON.parse(raw);
    if (
      data &&
      isLocalPath(data.executable) &&
      /^[\da-f]{64}$/.test(data.sha256) &&
      isToken(data.token) &&
      (data.stateDirectory === null || isLocalPath(data.stateDirectory))
    )
      return data;
  } catch {
    return null;
  }
  return null;
}

function parseReply(raw, exitCode) {
  let reply;
  try {
    reply = JSON.parse(raw);
  } catch {
    throw incompatible();
  }
  if (reply?.schema_version !== 1) throw incompatible();
  if (exitCode === 0 && Object.hasOwn(reply, 'result') && !Object.hasOwn(reply, 'error'))
    return reply.result;
  switch (reply.error?.code) {
    case 'not_authorized':
      throw new ClientError(
        'authorization',
        'Voldiq rejected the connection. Check its credential and permissions.',
      );
    case 'scan_cancelled':
      throw new ClientError('cancelled', 'The scan was cancelled.');
    case 'scan_failed':
      throw new ClientError(
        'scan_failed',
        'The scan did not complete. Inspect its job in Voldiq before retrying.',
      );
    case 'operation_failed':
      throw new ClientError(
        'unavailable',
        'Voldiq could not complete the operation. Check its folder permission and current jobs.',
      );
    default:
      throw incompatible();
  }
}

function readProgress(line) {
  try {
    const value = JSON.parse(line);
    if (value?.schema_version === 1 && value.state === 'running' && isId(value.job_id))
      return value.job_id;
  } catch {
    // Diagnostic stderr is not forwarded to VS Code.
  }
  return null;
}

function execute(connection, args, onJob) {
  const state = connection.stateDirectory === null ? [] : ['--state', connection.stateDirectory];
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([name]) => name.toUpperCase() !== 'VOLDIQ_AGENT_TOKEN'),
  );
  env.VOLDIQ_AGENT_TOKEN = connection.token;
  return new Promise((resolve, reject) => {
    const child = spawn(connection.executable, [...state, ...args], {
      cwd: path.dirname(connection.executable),
      env,
      windowsHide: true,
      shell: false,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const chunks = [];
    let length = 0;
    let progress = '';
    let discardLine = false;
    child.stdout.on('data', (chunk) => {
      length += chunk.length;
      if (length <= maximumOutput) chunks.push(chunk);
    });
    child.stderr.setEncoding('utf8');
    child.stderr.on('data', (chunk) => {
      if (!onJob) return;
      const parts = chunk.split('\n');
      for (let i = 0; i < parts.length; i++) {
        if (!discardLine && progress.length + parts[i].length <= maximumProgressLine)
          progress += parts[i];
        else discardLine = true;
        if (i === parts.length - 1) break;
        if (!discardLine) {
          const id = readProgress(progress);
          if (id !== null) onJob(id);
        }
        progress = '';
        discardLine = false;
      }
    });
    child.once('error', () =>
      reject(
        new ClientError('launch', 'Voldiq CLI could not start. Check the configured executable.'),
      ),
    );
    child.once('close', (code) => {
      try {
        if (length > maximumOutput || code === null) throw incompatible();
        resolve(parseReply(Buffer.concat(chunks).toString('utf8'), code));
      } catch (error) {
        reject(error);
      }
    });
  });
}

function makeClient(connection, checkLocation) {
  const invoke = async (args, onJob, signal) => {
    const actual = await identifyExecutable(connection.executable);
    await checkLocation(actual.executable);
    if (actual.executable !== connection.executable || actual.sha256 !== connection.sha256) {
      throw new ClientError(
        'executable',
        'The Voldiq CLI changed. Reconnect to confirm the new executable.',
      );
    }
    if (signal?.aborted) throw new ClientError('cancelled', 'The scan was cancelled.');
    return execute(connection, args, onJob);
  };
  return {
    async check() {
      const result = await invoke(['capabilities']);
      if (!Array.isArray(result?.capabilities)) throw incompatible();
      if (!result.capabilities.includes('metadata') || !result.capabilities.includes('scan')) {
        throw new ClientError(
          'authorization',
          'The connection needs metadata and scan access in Voldiq.',
        );
      }
      if (
        result.capabilities.some((capability) => capability !== 'metadata' && capability !== 'scan')
      ) {
        throw new ClientError(
          'authorization',
          'Create a dedicated Voldiq connection with metadata and scan access only.',
        );
      }
    },
    async analyze(folder, signal, report = () => {}) {
      if (!isLocalPath(folder)) throw new ClientError('scope', 'Choose a local Windows folder.');
      let jobId = null;
      let cancellation;
      const cancel = () => {
        if (jobId === null || cancellation) return;
        cancellation = invoke(['cancel', String(jobId)]).then(
          () => report('Cancellation requested…'),
          () => report('Cancellation could not be requested. Waiting for Voldiq…'),
        );
      };
      signal.addEventListener('abort', cancel);
      let job;
      try {
        job = await invoke(
          ['scan', folder, '--wait'],
          (id) => {
            if (jobId !== null) return;
            jobId = id;
            report(`Scanning · Job ${id}`);
            if (signal.aborted) cancel();
          },
          signal,
        );
      } finally {
        signal.removeEventListener('abort', cancel);
        await cancellation;
      }
      if (
        job?.state !== 'completed' ||
        !isId(job.id) ||
        !isId(job.snapshot_id) ||
        (jobId !== null && job.id !== jobId)
      )
        throw incompatible();
      const snapshot = job.snapshot_id;
      if (signal.aborted) return { kind: 'completed_while_cancelling', snapshot };
      report('Reading the saved snapshot…');
      const [stats, page] = await Promise.all([
        invoke(['stats', String(snapshot)]),
        invoke([
          'query',
          String(snapshot),
          JSON.stringify({ files_only: true, sort: 'size', limit: pageSize }),
        ]),
      ]);
      if (
        stats?.snapshot_id !== snapshot ||
        typeof stats.root !== 'string' ||
        typeof stats.incomplete !== 'boolean' ||
        !Array.isArray(page?.items) ||
        page.items.length > pageSize ||
        page.items.some((item) => typeof item?.name !== 'string')
      )
        throw incompatible();
      return { kind: 'report', job: job.id, snapshot, stats, items: page.items };
    },
  };
}

module.exports = {
  ClientError,
  identifyExecutable,
  isLocalPath,
  isToken,
  makeClient,
  sameOrInside,
  storedConnection,
};
