# Disk City implementation

The existing application uses Svelte 5, Tauri 2 and a Rust index. `query_overview` returns a bounded page of 250 children and up to twelve branches of eight children. Query ownership and cancellation already prevent stale navigation responses. Cleanup already has native preview and explicit approval; the visualization must not bypass it. There is no separate headless/MCP visualization service to extend.

The existing Canvas treemap lays out parent rectangles and draws children inside them. Disk City needs a disjoint frontier instead: a directory boundary can enclose child blocks, but cannot also contribute another building. Both views will consume one fixed 1000 × 700 layout, independent of window size, camera, colors and relief. Remainders retain measured bytes inside their own parent. Zero-byte entries remain in the paged list.

## Delivery units

1. Add the shared layout, deterministic palette and honest metric transformations, with conservation and unknown-value tests. Add cancellable descendant modification coverage to the existing bounded overview response.
2. Add orthographic prism geometry, spatial BSP subdivision and back-to-front face picking. Verify adversarial scenes against an independent ray/box reference.
3. Integrate Treemap / Disk City / List into the existing explorer and overview. Preserve scope, index identity, revision, selection, search and filters. Reuse the paged file table and inspector. Add cursor-centered zoom, pan threshold, fit, relief and keyboard controls.
4. Add coherent lighting, clipped receiver shadows, deterministic detail and demand rendering. Refine shared controls, sidebar and responsive inspector using existing themes and local fonts.
5. Run source gates, native Windows smoke tests, screenshots and reproducible rendering measurements. Record actual coverage and limitations, then publish version 0.2.0 with an installer and release notes.

## Decisions

- Canvas 2D only, no new runtime dependency or graphics engine.
- BSP splits crossing faces rather than guessing an object order. Its fragments retain their original node and face for selection and clipped decoration.
- Height starts with age since modification. Directories use the newest known descendant file modification, with coverage. Files, relative depth and age use documented scales; last access stays unavailable because the index has no reliability signal.
- File type is the initial color mode. Collapsed directories and measured remainders have explicit neutral classifications. Size and age are sequential alternatives.
- Unknown allocation and timestamps remain visible. Unique allocation is never described as guaranteed recoverable space.
- Preparation is bounded and generation-owned; switching the renderer never invokes a scan or reloads the tree. The existing list remains the keyboard-accessible route to the complete index.

## Review focus

Check partial directory sums, unknown allocation, long paths and zero bytes; test crossing projected facades and narrow rectangles; retain camera per scope; reject obsolete asynchronous work; verify graphic actions never execute a cleanup or file. Existing untracked `interface/prompts.json` and `output/` belong to the user and are excluded from commits.

## References

Reviewed [TreeMap Disk Visualizer](https://github.com/Prithvi-Web/TreeMap-Disk-Visualizer) as visual reference only. No source code or assets are copied. The implementation remains original to Voldiq. Technical references: [Canvas optimization](https://developer.mozilla.org/en-US/docs/Web/API/Canvas_API/Tutorial/Optimizing_canvas), [OffscreenCanvas](https://developer.mozilla.org/en-US/docs/Web/API/OffscreenCanvas), [Tauri frontend events](https://v2.tauri.app/develop/calling-frontend/), [Windows access-time policy](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/fsutil-behavior), [text contrast](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html), [use of color](https://www.w3.org/WAI/WCAG22/Understanding/use-of-color.html).
