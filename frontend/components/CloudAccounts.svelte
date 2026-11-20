<script lang="ts">
  import { isTauri } from '@tauri-apps/api/core';
  import CloudAccount from './CloudAccount.svelte';
</script>

<section aria-labelledby="cloud-accounts-heading">
  <h2 id="cloud-accounts-heading">Cloud accounts</h2>
  {#if isTauri()}
    <div>
      <CloudAccount provider="one_drive" />
      <CloudAccount provider="google_drive" />
      <CloudAccount provider="dropbox" />
    </div>
  {:else}
    <p>Cloud accounts are available in Voldiq for Windows.</p>
  {/if}
</section>
