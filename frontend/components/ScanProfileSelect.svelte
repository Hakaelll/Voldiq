<script lang="ts">
  import { scanProfiles, type ScanProfile } from '../lib/scan-profile';
  let {
    profile = $bindable<ScanProfile>('auto'),
    disabled = false,
    compact = false,
    onchange,
  }: {
    profile: ScanProfile;
    disabled?: boolean;
    compact?: boolean;
    onchange: () => void;
  } = $props();
  const description = $derived(scanProfiles.find((item) => item.id === profile)?.description);
</script>

<div class="profile-control" class:compact>
  <label
    ><span class:sr-only={compact}>Scan profile</span>
    <select bind:value={profile} {disabled} {onchange} title={description}>
      {#each scanProfiles as option}<option value={option.id}>{option.label}</option>{/each}
    </select>
  </label>
  {#if !compact}<p>{description} Your concurrency limit still applies.</p>{/if}
</div>

<style>
  .profile-control {
    min-width: 0;
  }
  label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  select {
    min-width: 132px;
  }
  p {
    color: var(--secondary);
    font-size: 12px;
    line-height: 1.45;
    margin: 6px 0 0;
  }
  .compact select {
    min-width: 120px;
    max-width: 150px;
  }
  .profile-control:not(.compact) label {
    display: grid;
    grid-template-columns: 260px minmax(0, 1fr);
    gap: 20px;
  }
  .profile-control:not(.compact) p {
    margin-left: 280px;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
    border: 0;
  }
  @media (max-width: 840px) {
    .profile-control:not(.compact) label {
      grid-template-columns: 1fr;
      gap: 8px;
    }
    .profile-control:not(.compact) p {
      margin-left: 0;
    }
  }
</style>
