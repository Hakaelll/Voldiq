<script lang="ts">
  let { name, size = 18 }: { name: string; size?: number } = $props();
  const paths: Record<string, string> = {
    overview: 'M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z',
    folder: 'M3 7V5h6l2 2h10v13H3z',
    scan: 'M20 7a8 8 0 1 0 1 9 M20 3v5h-5',
    search: 'M21 21l-5-5 M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0',
    large: 'M6 3h8l4 4v14H6z M14 3v5h4 M9 12h6 M9 16h6',
    settings:
      'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8 M12 2v3 M12 19v3 M2 12h3 M19 12h3 M5 5l2 2 M17 17l2 2 M5 19l2-2 M17 7l2-2',
    collapse: 'M15 5l-7 7 7 7',
    expand: 'M9 5l7 7-7 7',
    file: 'M6 3h8l4 4v14H6z M14 3v5h4',
    back: 'M20 12H4 M10 6l-6 6 6 6',
    disk: 'M5 4h14l3 12v4H2v-4z M2 16h20 M16 18h1 M19 18h1',
    warning: 'M12 3l10 18H2z M12 9v5 M12 17v1',
    close: 'M6 6l12 12 M18 6L6 18',
    copy: 'M8 8h13v13H8z M16 8V3H3v13h5',
    external: 'M14 3h7v7 M21 3l-11 11 M10 3H3v18h18v-7',
    list: 'M8 6h13 M8 12h13 M8 18h13 M3 6h1 M3 12h1 M3 18h1',
    history: 'M3 11a9 9 0 1 1 2 7 M3 3v8h8 M12 7v6l4 2',
    trash: 'M3 6h18 M8 6V3h8v3 M5 6l1 15h12l1-15 M10 10v7 M14 10v7',
    duplicate: 'M8 8h13v13H8z M16 8V3H3v13h5',
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.6"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"><path d={paths[name] ?? paths.file} /></svg
>
