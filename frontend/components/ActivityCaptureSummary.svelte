<script lang="ts">
  import { utcDate } from '../lib/history-activity';
  import { bytes, type ActivityCapture } from '../lib/types';
  let { capture }: { capture: ActivityCapture } = $props();
</script>

<div class="activity-capture-summary">
  <span>Capture #{capture.id} · saved {utcDate(capture.captured)}</span>
  <span
    >{capture.observation
      ? `Measured ${utcDate(capture.observation.completed_at)}`
      : 'Measurement date unavailable'}</span
  >
  <span
    >{capture.complete ? 'Recorded complete coverage' : 'Partial coverage'} · {capture.accounting ===
    'supported'
      ? 'Known accounting revision'
      : capture.accounting === 'unknown'
        ? 'Legacy accounting unverified'
        : 'Unsupported accounting revision'}</span
  >
  <span title={`${capture.logical} recorded logical bytes`}
    >Root logical size · {capture.complete ? '' : 'At least '}{bytes(Number(capture.logical))}</span
  >
</div>
