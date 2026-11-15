<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import { applicationBytes as formatBytes } from '../lib/applications';
  import { trayPercent } from '../lib/cleanup-tray';
  import { date, type Volume } from '../lib/types';

  type Sample = { total: string; used: string; free: string; observed_at: number | null };
  type Host = {
    endpoint: string;
    running: boolean;
    drive: string;
    devices: number[];
    sample: Sample;
    invitation_seconds: number;
  };
  type Invitation = { code: string; lifetime_seconds: number };
  type Remote = { device: number; endpoint: string; sample: Sample };
  type View = { host: Host | null; remotes: Remote[] };
  type Response = View & { invitation: Invitation | null };
  type Action =
    | { kind: 'status' | 'stop' | 'create_invitation' | 'update_shared_summary' }
    | { kind: 'start'; address: string; drive: string }
    | { kind: 'join'; code: string }
    | { kind: 'refresh' | 'revoke' | 'forget'; device: number };

  let { volumes }: { volumes: Volume[] } = $props();

  const native = isTauri();
  let eligible = $derived(
    volumes.filter(
      (volume) =>
        volume.accessible &&
        volume.space.kind === 'measured' &&
        (volume.drive_type === 'Local' || volume.drive_type === 'Removable'),
    ),
  );
  let open = $state(false);
  let view = $state<View | null>(null);
  let invitation = $state<Invitation | null>(null);
  let revealed = $state(false);
  let code = $state('');
  let drive = $state('');
  let address = $state('');
  let addresses = $state<string[]>([]);
  let busy = $state<Action['kind'] | null>(null);
  let problem = $state('');
  let message = $state('');
  let showJoin = $state(false);
  let revision = 0;
  let panelSummary = $state<HTMLElement>();
  let shareHeading = $state<HTMLHeadingElement>();
  let joinButton = $state<HTMLButtonElement>();
  let selectedDrive = $derived(
    eligible.some((volume) => volume.path === drive) ? drive : (eligible[0]?.path ?? ''),
  );

  const command = (action: Action) => invoke<Response>('lan_sharing', { action });

  onMount(() => {
    if (!native) return;
    const requested = revision;
    void command({ kind: 'status' })
      .then(({ host, remotes }) => {
        if (requested === revision) view = { host, remotes };
      })
      .catch(() => {});
  });

  function hideInvitation(): void {
    invitation = null;
    revealed = false;
  }

  async function load(): Promise<void> {
    if (!native || busy) return;
    revision++;
    busy = 'status';
    problem = '';
    try {
      const { host, remotes } = await command({ kind: 'status' });
      view = { host, remotes };
      addresses = await invoke<string[]>('local_network_addresses');
      if (!addresses.includes(address)) address = addresses[0] ?? '';
      if (!eligible.some((volume) => volume.path === drive)) drive = eligible[0]?.path ?? '';
    } catch (error) {
      problem = String(error);
    } finally {
      busy = null;
    }
  }

  async function perform(action: Action): Promise<void> {
    if (!native || busy) return;
    revision++;
    busy = action.kind;
    problem = '';
    message = '';
    try {
      const { host, remotes, invitation: nextInvitation } = await command(action);
      view = { host, remotes };
      if (action.kind === 'create_invitation') {
        invitation = open ? nextInvitation : null;
        revealed = false;
      }
      if (action.kind === 'join') {
        code = '';
        showJoin = false;
        message = 'Device connected.';
      } else if (action.kind === 'stop') {
        hideInvitation();
        message = 'Sharing stopped.';
      } else if (action.kind === 'revoke') {
        message = 'Device access revoked.';
      } else if (action.kind === 'update_shared_summary' || action.kind === 'refresh') {
        message = 'Storage summary updated.';
      }
    } catch (error) {
      problem = String(error);
    } finally {
      busy = null;
    }
    if (!open || problem) return;
    await tick();
    if (action.kind === 'start' || action.kind === 'revoke') shareHeading?.focus();
    else if (action.kind === 'stop') panelSummary?.focus();
    else if (action.kind === 'join' || action.kind === 'forget') joinButton?.focus();
  }

  async function copyInvitation(): Promise<void> {
    if (!invitation || busy) return;
    try {
      await navigator.clipboard.writeText(invitation.code);
      message = 'Invitation copied.';
    } catch {
      revealed = true;
      message = 'Select and copy the invitation.';
    }
  }

  let pollActive = $derived(open && view?.host?.running === true);
  $effect(() => {
    if (!pollActive) return;
    const timer = window.setInterval(async () => {
      if (busy) return;
      const requested = revision;
      try {
        const { host, remotes } = await command({ kind: 'status' });
        if (busy || !open || requested !== revision) return;
        view = { host, remotes };
        if (!host?.invitation_seconds) hideInvitation();
      } catch (error) {
        if (!busy && open && requested === revision) problem = String(error);
      }
    }, 1200);
    return () => window.clearInterval(timer);
  });
</script>

{#snippet storage(sample: Sample)}
  <div class="sample">
    <div class="capacity" aria-hidden="true">
      <span style:width={`${trayPercent(sample.used, sample.total)}%`}></span>
    </div>
    <dl>
      <div>
        <dt>Used</dt>
        <dd>{formatBytes(sample.used)}</dd>
      </div>
      <div>
        <dt>Free</dt>
        <dd>{formatBytes(sample.free)}</dd>
      </div>
      <div>
        <dt>Capacity</dt>
        <dd>{formatBytes(sample.total)}</dd>
      </div>
    </dl>
    <small
      >{sample.observed_at === null
        ? 'Measurement time unavailable'
        : `Measured ${date(sample.observed_at)}`}</small
    >
  </div>
{/snippet}

<section aria-labelledby="lan-heading">
  <h2 id="lan-heading">Local network</h2>
  <details
    ontoggle={(event) => {
      revision++;
      open = event.currentTarget.open;
      if (open) void load();
      else {
        hideInvitation();
        showJoin = false;
        code = '';
      }
    }}
  >
    <summary bind:this={panelSummary}>
      <span>Storage sharing</span>
      {#if native && view}<small>{view.host?.running ? 'Sharing' : 'Sharing off'}</small>{/if}
    </summary>
    {#if !native}
      <p>Local network sharing is available in the Windows app.</p>
    {:else}
      <div class="lan-content">
        {#if view?.host}
          {@const host = view.host}
          <div class="share-head">
            <div>
              <h3 bind:this={shareHeading} tabindex="-1">
                {host.running ? 'Sharing' : 'Share unavailable'}
                {host.drive}
              </h3>
              <small>{host.endpoint}</small>
            </div>
            <button
              class="quiet-action"
              disabled={busy !== null}
              onclick={() => void perform({ kind: 'stop' })}>Stop sharing</button
            >
          </div>
          {@render storage(host.sample)}
          <div class="actions">
            <button
              disabled={!host.running || busy !== null || host.devices.length >= 8}
              onclick={() => void perform({ kind: 'create_invitation' })}
            >
              {host.invitation_seconds > 0 ? 'Replace invitation' : 'New invitation'}
            </button>
            <button
              class="quiet-action"
              disabled={!host.running || busy !== null}
              onclick={() => void perform({ kind: 'update_shared_summary' })}
              >Update shared summary</button
            >
          </div>
          {#if host.invitation_seconds > 0}
            <div class="invite">
              <p>
                One device can join in the next {host.invitation_seconds} seconds. Send the invitation
                privately.
              </p>
              {#if invitation}
                <div class="actions">
                  <button class="quiet-action" onclick={() => void copyInvitation()}
                    >Copy invitation</button
                  >
                  <button
                    class="quiet-action"
                    aria-expanded={revealed}
                    onclick={() => (revealed = !revealed)}
                  >
                    {revealed ? 'Hide code' : 'Show code'}
                  </button>
                </div>
                {#if revealed}<code>{invitation.code}</code>{/if}
              {/if}
            </div>
          {/if}
          {#if host.devices.length}
            <ul class="device-list" aria-label="Devices with access">
              {#each host.devices as device (device)}
                <li>
                  <span>Paired device {device}</span>
                  <button
                    class="quiet-action"
                    disabled={busy !== null}
                    aria-label={`Revoke access for paired device ${device}`}
                    onclick={() => void perform({ kind: 'revoke', device })}>Revoke access</button
                  >
                </li>
              {/each}
            </ul>
          {/if}
        {:else if view}
          <p>
            Share a drive’s capacity, used space, free space and measurement time with an invited
            device. Sharing ends when you quit Voldiq.
          </p>
          {#if addresses.length && eligible.length}
            <div class="share-options">
              <label
                >Drive <select
                  value={selectedDrive}
                  onchange={(event) => (drive = event.currentTarget.value)}
                  disabled={busy !== null}
                >
                  {#each eligible as volume (volume.path)}
                    <option value={volume.path}>{volume.path} {volume.name}</option>
                  {/each}
                </select></label
              >
              {#if addresses.length > 1}
                <label
                  >Local address <select bind:value={address} disabled={busy !== null}>
                    {#each addresses as ip (ip)}<option value={ip}>{ip}</option>{/each}
                  </select></label
                >
              {/if}
              <button
                disabled={busy !== null || !selectedDrive || !address}
                onclick={() => void perform({ kind: 'start', drive: selectedDrive, address })}
                >Start sharing</button
              >
            </div>
          {:else if busy === null}
            <p>
              {addresses.length === 0
                ? 'Connect to a private IPv4 network to share from this computer.'
                : 'A local drive with a measured capacity is required.'}
            </p>
            <button class="quiet-action" onclick={() => void load()}>Check again</button>
          {/if}
        {/if}
        {#if view}
          <div class="other-devices">
            <div class="share-head">
              <h3>Other devices</h3>
              <button
                bind:this={joinButton}
                class="quiet-action"
                disabled={busy !== null || view.remotes.length >= 8}
                aria-expanded={showJoin}
                onclick={() => {
                  showJoin = !showJoin;
                  if (!showJoin) code = '';
                }}
              >
                {showJoin ? 'Cancel' : 'Join with invitation'}
              </button>
            </div>
            {#if showJoin}
              <form
                class="share-options"
                onsubmit={(event) => {
                  event.preventDefault();
                  if (code.trim()) void perform({ kind: 'join', code });
                }}
              >
                <label
                  >Invitation <input
                    type="password"
                    bind:value={code}
                    maxlength="120"
                    autocomplete="off"
                    spellcheck="false"
                    disabled={busy !== null}
                    required
                  /></label
                >
                <button disabled={busy !== null || !code.trim()}>Connect</button>
              </form>
            {/if}
            {#if view.remotes.length}
              <ul class="device-list">
                {#each view.remotes as remote (remote.device)}
                  <li class="remote-device">
                    <div class="share-head">
                      <div>
                        <strong>Device {remote.device}</strong> <small>{remote.endpoint}</small>
                      </div>
                      <div class="actions">
                        <button
                          class="quiet-action"
                          disabled={busy !== null}
                          aria-label={`Refresh device ${remote.device}`}
                          onclick={() => void perform({ kind: 'refresh', device: remote.device })}
                          >Refresh</button
                        >
                        <button
                          class="quiet-action"
                          disabled={busy !== null}
                          aria-label={`Forget device ${remote.device}`}
                          onclick={() => void perform({ kind: 'forget', device: remote.device })}
                          >Forget</button
                        >
                      </div>
                    </div>
                    {@render storage(remote.sample)}
                  </li>
                {/each}
              </ul>
            {:else}
              <p>No devices joined in this session.</p>
            {/if}
          </div>
        {/if}
        <div role="status">
          {busy === 'status'
            ? 'Checking local sharing…'
            : busy === 'join'
              ? 'Connecting…'
              : message}
        </div>
        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
        {#if !view && busy === null}<button class="quiet-action" onclick={() => void load()}
            >Retry</button
          >{/if}
      </div>
    {/if}
  </details>
</section>

<style>
  details {
    border-block: 1px solid var(--border);
  }
  summary {
    cursor: pointer;
    padding-block: 10px;
    font-weight: 600;
  }
  summary small {
    margin-inline-start: 8px;
    font-weight: 400;
  }
  small,
  dt {
    color: var(--secondary);
  }
  .lan-content {
    display: grid;
    gap: 9px;
    padding: 0 0 10px 17px;
  }
  p,
  h3,
  ul,
  dl {
    margin: 0;
  }
  h3 {
    font-size: 14px;
  }
  .share-head,
  .actions,
  .share-options,
  dl,
  .device-list li:not(.remote-device) {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 14px;
    min-width: 0;
  }
  .share-head,
  .device-list li:not(.remote-device) {
    justify-content: space-between;
  }
  .share-options {
    align-items: end;
  }
  .share-options label {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  input,
  select {
    max-width: 100%;
  }
  .sample {
    display: grid;
    gap: 5px;
    min-width: 0;
  }
  .capacity {
    background: var(--border);
    overflow: hidden;
    height: 5px;
    border-radius: 3px;
  }
  .capacity span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  dt {
    font-size: 12px;
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .device-list {
    list-style: none;
    padding: 0;
  }
  .device-list li,
  .other-devices {
    border-top: 1px solid var(--border);
    padding-block: 7px;
  }
  .invite,
  .other-devices,
  .remote-device {
    display: grid;
    gap: 6px;
  }
  code,
  .problem {
    overflow-wrap: anywhere;
  }
  code {
    user-select: text;
  }
  .problem {
    color: var(--error);
  }
</style>
