<script lang="ts">
  import artwork from '../../src-tauri/icons/256x256.png';
  let { size = 40 }: { size?: number } = $props();
</script>

<span class="brand-mark" style:--mark-size={`${size}px`} aria-hidden="true">
  <img src={artwork} alt="" width="256" height="256" />
</span>

<style>
  .brand-mark {
    display: inline-block;
    position: relative;
    flex-shrink: 0;
    width: var(--mark-size);
    height: var(--mark-size);
    overflow: hidden;
    border-radius: 20%;
    vertical-align: middle;
  }
  img {
    position: absolute;
    width: 160%;
    height: 160%;
    max-width: none;
    left: -30%;
    top: -30%;
  }
</style>
