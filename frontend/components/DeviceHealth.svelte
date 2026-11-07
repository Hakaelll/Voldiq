<script lang="ts">
  import type { DeviceHealthReport, HealthUnavailable } from '../lib/device-health';
  import type { IndexId } from '../lib/index-context';
  import { utcDate } from '../lib/history-activity';
  import { deviceHealthQueries } from '../lib/queries';

  let { indexId, paused }: { indexId: IndexId; paused: boolean } = $props();
  type Model =
    | { kind: 'idle' | 'loading' | 'cancelled' | 'failed' }
    | { kind: 'ready'; report: DeviceHealthReport };
  let model = $state.raw<Model>({ kind: 'idle' });
  let request: ReturnType<typeof deviceHealthQueries.request> | null = null;
  const reasons: Record<HealthUnavailable, string> = {
    recorded_scan: 'Run a fresh scan to check the disk behind this folder.',
    root_changed: 'The scanned folder could not be matched. Scan again.',
    volume_unmapped: 'A local Windows volume could not be identified.',
    extent_limit: 'The volume exceeds the supported disk-mapping limit.',
    mapping_changed: 'The disk mapping changed during the query. Check again.',
    access_denied: 'Windows denied access for the current account.',
    unsupported: 'The controller or driver did not expose a supported health report.',
    invalid_response: 'Windows returned incomplete or unrecognized device data.',
    driver_error: 'Windows could not complete the device query.',
  };

  $effect(() => {
    indexId;
    paused;
    model = { kind: 'idle' };
    return () => {
      request?.cancel();
      request = null;
    };
  });

  async function check(): Promise<void> {
    if (paused) return;
    if (request) {
      request.cancel();
      request = null;
      model = { kind: 'cancelled' };
      return;
    }
    model = { kind: 'loading' };
    const job = deviceHealthQueries.request({ indexId });
    request = job;
    try {
      const result = await job.result;
      if (request === job)
        model = result ? { kind: 'ready', report: result } : { kind: 'cancelled' };
    } catch {
      if (request === job) model = { kind: 'failed' };
    } finally {
      if (request === job) request = null;
    }
  }

  const counter = (value: string) => BigInt(value).toLocaleString();
</script>

<section class="type-panel device-health" aria-labelledby="device-health-heading">
  <div class="panel-heading">
    <h2 id="device-health-heading">Drive health</h2>
    <button disabled={paused} onclick={check}>
      {model.kind === 'loading'
        ? 'Cancel check'
        : model.kind === 'idle'
          ? 'Check drive'
          : 'Check again'}
    </button>
  </div>

  {#if paused}
    <p>Available after the current operation.</p>
  {:else if model.kind === 'idle'}
    <p>Read device health data for the volume that contains the scanned folder.</p>
  {:else if model.kind === 'loading'}
    <p role="status">Waiting for the Windows storage driver…</p>
  {:else if model.kind === 'cancelled'}
    <p role="status">Check cancelled.</p>
  {:else if model.kind === 'failed'}
    <p role="alert">Drive health could not be queried. Check again to retry.</p>
  {:else if model.kind === 'ready'}
    {@const { root, checked_at, observation } = model.report}
    <p class="health-context" role="status">
      {root} · {checked_at === null ? 'Check time unavailable' : `Checked ${utcDate(checked_at)}`}
    </p>
    {#if observation.kind === 'unavailable'}
      <p>{reasons[observation.reason]}</p>
    {:else}
      {#each observation.items as { number, health } (number)}
        <div class="disk-health">
          <h3>Windows disk {number}</h3>
          {#if health.kind === 'nvme'}
            <p>NVMe SMART / health log</p>
            <dl>
              <div>
                <dt>Critical warning flags</dt>
                <dd>
                  {health.critical_warning
                    ? `0x${health.critical_warning.toString(16).padStart(2, '0')}`
                    : 'None reported'}
                </dd>
              </div>
              <div>
                <dt>Composite temperature</dt>
                <dd>
                  {health.temperature_kelvin === null
                    ? 'Unreported'
                    : `${(health.temperature_kelvin - 273.15).toFixed(0)} °C`}
                </dd>
              </div>
              <div>
                <dt>Power-on hours</dt>
                <dd>{counter(health.power_on_hours)}</dd>
              </div>
              <div>
                <dt>Unsafe shutdowns</dt>
                <dd>{counter(health.unsafe_shutdowns)}</dd>
              </div>
              <div>
                <dt>Media and data integrity errors</dt>
                <dd>{counter(health.media_errors)}</dd>
              </div>
            </dl>
          {:else if health.kind === 'windows_prediction'}
            <p>
              Windows failure-prediction status · {health.alert
                ? 'Device reported an alert'
                : 'No alert reported'}
            </p>
          {:else}
            <p>{reasons[health.reason]}</p>
          {/if}
        </div>
      {/each}
      <p>
        Controllers can hide underlying drives. These readings cannot estimate remaining drive life.
      </p>
    {/if}
  {/if}
</section>

<style>
  .device-health {
    min-width: 0;
  }
  p {
    margin: 4px 0 0;
    color: var(--secondary);
  }
  .health-context {
    overflow-wrap: anywhere;
  }
  .disk-health {
    margin: 12px 0;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }
  h3 {
    margin: 0;
    font-size: 14px;
  }
  dl {
    margin: 12px 0 0;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(170px, 100%), 1fr));
    gap: 12px 20px;
  }
  dt {
    font-size: 12px;
    color: var(--secondary);
  }
  dd {
    margin: 2px 0 0;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
</style>
