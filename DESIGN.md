---
name: Voldiq
description: Compact Windows storage analysis with measured data and a white workspace.
colors:
  accent: "#0057d9"
  accent-fill: "#0057d9"
  accent-hover: "#173784"
  selected: "#e9edf2"
  canvas: "#fff"
  surface: "#f7f8fa"
  paper: "#edf0f5"
  border: "#e5eaf1"
  strong: "#d4d4d4"
  text: "#151c36"
  secondary: "#4f5d7b"
  muted: "#5c6984"
  error: "#a32424"
  dark-canvas: "#181b20"
  dark-surface: "#1e2228"
  dark-paper: "#252a31"
  dark-border: "#343a43"
  dark-strong: "#4b535f"
  dark-text: "#e6e8ec"
  dark-secondary: "#b8bec8"
  dark-muted: "#a2aab6"
  dark-accent: "#83aeff"
  dark-accent-fill: "#315ba6"
  dark-selected: "#263c60"
  dark-error: "#f29797"
  map-blue: "#075bdd"
  map-navy: "#123f86"
  map-teal: "#087f8c"
  map-orange: "#ed701b"
  map-ochre: "#c59127"
  map-slate: "#717a84"
typography:
  headline:
    fontFamily: "Segoe UI Variable, Segoe UI, sans-serif"
    fontSize: "30px"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "-0.02em"
  title:
    fontFamily: "Segoe UI Variable, Segoe UI, sans-serif"
    fontSize: "17px"
    fontWeight: 600
    lineHeight: 1.4
  body:
    fontFamily: "Segoe UI Variable, Segoe UI, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.6
  label:
    fontFamily: "Segoe UI Variable, Segoe UI, sans-serif"
    fontSize: "12px"
    fontWeight: 400
  metadata:
    fontFamily: "Segoe UI Variable, Segoe UI, sans-serif"
    fontSize: "11px"
    fontWeight: 400
  metric:
    fontFamily: "Segoe UI Variable, Segoe UI, sans-serif"
    fontSize: "23px"
    fontWeight: 600
    letterSpacing: "-0.02em"
rounded:
  radius: "3px"
  inset: "3px"
  dialog: "8px"
spacing:
  tight: "4px"
  small: "8px"
  control-gap: "10px"
  medium: "12px"
  panel-gap: "16px"
  panel-inset: "18px"
  compact-page: "20px"
  section-gap: "22px"
  page: "26px"
components:
  button-primary:
    backgroundColor: "{colors.accent-fill}"
    textColor: "{colors.canvas}"
    rounded: "{rounded.radius}"
    padding: "6px 10px"
    height: "32px"
  button-primary-hover:
    backgroundColor: "{colors.accent-hover}"
    textColor: "{colors.canvas}"
  button-secondary:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.text}"
    rounded: "{rounded.radius}"
    padding: "6px 10px"
    height: "32px"
  search:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.text}"
    rounded: "{rounded.radius}"
    padding: "9px 10px"
    width: "390px"
  navigation-active:
    backgroundColor: "{colors.selected}"
    textColor: "{colors.accent}"
    rounded: "{rounded.radius}"
    padding: "10px 12px"
  panel:
    backgroundColor: "{colors.canvas}"
    rounded: "{rounded.radius}"
  field:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.text}"
    rounded: "{rounded.radius}"
    padding: "7px 9px"
  file-row:
    typography: "{typography.label}"
    padding: "0 16px"
    height: "36px"
  notice:
    backgroundColor: "{colors.surface}"
    typography: "{typography.label}"
    rounded: "{rounded.radius}"
    padding: "10px 12px"
  history-controls:
    typography: "{typography.label}"
  history-row:
    typography: "{typography.body}"
    padding: "12px 16px"
    height: "68px"
---

# Design System: Voldiq

## Overview

The unscanned Overview pairs a drive summary with the scan controls. The selected drive's measured used percentage forms a ring above its used, free and total bytes. The supplied symbol, primary scan action, folder picker and up to three saved recent paths occupy the second column. The layout stacks on narrow windows and contains long volume names. It adds no illustrative disk values or explanatory paragraphs. Storage-map folder headings are larger than child labels; separators stay within cells that have room for them.

Populated maps show each sufficiently wide root region's percentage of the rendered measured weights, using the selected size metric. Nested children are excluded from this denominator to avoid counting the same bytes twice. The selection caption pairs its path and selected size with its indexed file count. File-type legends use responsive columns with aligned swatches and measured size/count labels.

Explorer rows now pair their logical sizes with compact bars scaled to the largest item in the complete filtered result, retaining the virtual row geometry. Folder details show a 48px composition strip for the five measured direct children and the parent remainder. Child colors match the actionable list below. The strip is omitted when child totals exceed the indexed parent size; it never fills missing measurements with sample values.

Developer storage uses a 44px measured category strip with bytes, percentages and directory counts below. Its category legends are filter buttons with pressed states; choosing a category twice or choosing All categories clears that filter. The legend and reset/order controls remain mounted through loading, while interaction is disabled. Category totals continue to describe the complete deduplicated result rather than the currently filtered page.

Duplicate group rows include compact meters relative to the report's total potential allocation. The active group's copy-balance strip compares its unselected and selected copy counts across member pages. Each member identifies Keep or Selected for review, and selected rows use the shared selection surface. These are selection states, not deletion results or verified retained-file authority; the native review remains mandatory.

History compares logical file changes with three bars on a shared signed scale around a central zero line. Added bytes extend right, removed bytes left, and Changed uses its complete signed net difference. Values and file counts remain readable beside each bar, including zero-byte changes. Newly observed and unobserved coverage stay separate and do not affect this scale. No extra chart runtime is loaded.

**Creative North Star: "The Windows storage workspace"**

Voldiq puts paths, file sizes and scan coverage in a compact desktop workspace. The built interface follows the user-selected white Windows direction and supplied Palantir, Linear and Dub references. Fine dividers organize the rail, toolbar and content without decorative elevation. Segoe UI keeps the controls consistent with Windows expectations.

The supplied stacked-storage logo is the identity asset. Its disc-shaped top and blue segment remain intact. Light mode is the default; dark mode changes the same semantic color roles. The dark navigation logo has an intentional pale circular backing, with a small inset, so the original raster remains legible without exposing a rectangular matte.

**Key Characteristics:**

- A single navigation rail expands to show labels.
- White content areas and fine neutral dividers support compact controls.
- Blue marks selection, focus and primary actions.
- Measured storage data determines chart area and totals.
- The original Voldiq logo remains visible in both themes.

This document records the initial built UI in `frontend/styles.css`, `frontend/App.svelte` and the component files. Finish approval covers the scored dark-logo matte and contrast corrections plus the initial browser empty and settings views. Populated native views, scan progress, IPC, the Canvas map, the virtual file list and access-issue dialog remain unassessed in a running Windows session. The mechanical detector reported no findings and the shipping raster provenance scanner reported zero missing entries; neither result proves those native workflows.

History extends this system with saved-snapshot controls and compact comparisons. Earlier desktop light, desktop dark and minimum-window dark screenshots showed its browser empty state. The subsequent loading, unavailable and deletion-focus changes were checked in source only. Saving, opening, deleting, compacting and comparing populated native snapshots remain unassessed.

## Colors

The palette uses white and neutral gray for structure, blue for interaction, and muted category colors for the storage map. Frontmatter records exact source values. Each `dark-` token replaces its matching light role under the root dark-theme attribute.

### Primary

- **Selection blue** uses `accent` for current navigation, focus and carets; `selected` supplies the pale selection background.
- **Action blue** uses `accent-fill` behind primary button text. `accent-hover` is the actual shared hover value in both themes.

### Secondary

- **Measured map colors** use the six `map-` tokens. Root regions receive colors in size-rank order, with later regions using slate. Nested children inherit their enclosing region's fill and paired text color. Largest-item swatches match these regions. File-type summaries retain name-based colors. Neither palette encodes a semantic file-type taxonomy.

### Neutral

- **White canvas** uses `canvas` for the workspace, ordinary buttons and panels.
- **Quiet containers** use `surface` for the rail, empty state, notices and table headers. `paper` supports hover and inset backgrounds.
- **Fine structure** uses `border` for dividers and `strong` for fields, tooltips and dialog outlines.
- **Reading hierarchy** uses `text`, `secondary` and `muted` for primary content, explanation and metadata.
- **Error text** uses `error` within the same neutral notice container.

**The Semantic Theme Rule.** Use the existing semantic custom properties for new controls so theme changes update their text, background, border and focus together.

## Typography

The UI uses Segoe UI Variable with Segoe UI and sans-serif fallbacks. It has no separate display face. Canvas text uses Segoe UI explicitly. Use the frontmatter roles as the recorded hierarchy; paragraph body text has the body line height, while controls keep their native inherited line height.

Page headings use `headline`, panel and settings headings use `title`, paragraphs and controls use `body`, dense file rows and descriptions use `label`, and footer text and small annotations use `metadata`. The unscanned capacity strip uses `metric`, dropping to 20px at the first desktop breakpoint; populated Overview uses compact 13px label/value rows. The welcome heading is a local 24px variation, and the brand wordmark uses 27px or 24px at the first breakpoint.

**The Measured Number Rule.** Keep storage sizes and file counts aligned with tabular numerals. Right-align the file list's logical-size column; allow names and paths to wrap or truncate according to their container.

## Layout

The desktop shell starts with a collapsed rail of 64px and expands to 216px while the pointer is inside navigation. It collapses on pointer leave, while visible keyboard focus retains access to labels. Pointer and keyboard activity update focus modality even on the same button. Expansion is transient, with no manual toggle or saved expansion preference. The rail fills the viewport with its own overflow behavior. The toolbar has a 66px minimum height and can wrap. Main content uses the page spacing token. Settings stops at 840px, places 260px labels beside controls with a 20px gap, and limits select widths to 286px. Appearance, Scanning and Index have separate horizontal dividers. Light and Dark are native radio choices with a visible focus outline; motion remains a native checkbox. At 840px and below, labels stack above controls and action alignment resets to the left edge.

The populated overview follows `interface/01-overview-root.png` and `02-overview-folder.png`: a 248px summary column with measured capacity, index totals and largest items, followed by a dominant map and file-type legend. The summary narrows to 210px below 1100px; below 840px the sections stack without hiding largest items. Map height scales from 360px to 600px. The capacity strip remains only in the unscanned overview. Other data views use their shared header without an extra capacity strip. The file list retains its 36px logical rows and existing bounded virtualization.

History uses a 12px control gap and 18px spacing below its toolbar. Snapshot rows have a 68px minimum height, medium vertical padding and panel-gap horizontal padding. Before and After selects wrap with a 180px minimum label width. Its recorded-size graphic is 110px high and uses the existing accent color.

At 1100px and below, the expanded rail remains 216px, main padding becomes the compact-page token, search narrows to 280px, and the overview becomes one column with Largest items hidden. At 840px and below, the layout reserves a 64px rail and expanded navigation overlays the content at 216px, the toolbar wraps, search occupies a separate row, capacity becomes two columns, and settings labels stack above their controls. This is a desktop window adaptation; a phone layout has not been assessed.

**The One Rail Rule.** Expand labels within the existing navigation rail. Keep the workspace as the adjacent main area.

## Elevation & Depth

The built interface uses no box shadows. One-pixel borders and adjacent neutral backgrounds separate panels, fields and navigation. The access-issue dialog uses a fixed black backdrop at 25% opacity and a stronger border. Its depth comes from occlusion and the backdrop.

Buttons transition their background over the root motion duration of 140ms. Rail width uses the same duration with ease-out. The system reduced-motion preference and the saved Reduce motion setting suppress transitions and animations.

**The Border Depth Rule.** Use the existing border and background roles to organize content. Keep panel depth consistent with the flat built containers.

## Shapes

Controls, panels, notices and selection backgrounds use the radius token. Smaller chart insets and keyboard hints use the inset radius. The dialog uses the dialog radius. Table rows use square corners and bottom dividers.

Icons are inline outline SVGs, generally 18px, with a 24-unit viewbox, 1.6-unit current-color stroke and round caps and joins. The navigation's dark-theme logo backing is circular, pale paper, and inset by 3px inside its 32px image box. This is a deliberate treatment for the supplied identity raster, not a general button or card shape.

## Components

### Buttons

Primary actions use action blue and white text. Secondary controls use the canvas, text and border roles. Both use a minimum height of 32px, compact padding and an 8px icon gap. Hover changes the background; disabled controls use 50% opacity and a default cursor. Keyboard focus uses a 2px accent outline with a 2px offset.

### Inputs / Fields

Text fields, selects and textareas use a stronger neutral border and compact insets. The search control has one enclosing border, an outline icon and a keyboard hint. Focus belongs to the enclosing search container so its inner input does not draw a second ring. Settings controls retain native Windows select and checkbox behavior.

### Navigation

The rail groups Overview, Explorer and Large files at the top and Settings at the bottom. Active items use the selected background, accent text and semibold weight. Hover uses paper. Collapsed controls retain titles and accessible names. The same controls and SVG geometry serve both rail widths.

History adds a rail destination with the same current-page, icon and collapsed-label treatment.

### Cards / Containers

Panels have a fine border, canvas background and the standard control radius. Panel headings use compact insets with a title, optional explanation and optional action. The initial empty container centers the supplied logo, heading, paragraph and actions within a 430px minimum height. Its explanatory paragraph stops at 460px.

### Storage map and capacity

The populated summary aligns measured capacity and index figures in label/value rows, with a labelled capacity meter. The map sizes rectangles from actual logical or allocated bytes, with small gaps between regions. Selection uses an outline and hover exposes a neutral bordered tooltip. Map/List controls and a local metric selector retain real navigation and saved metric behavior. The Canvas implementation and its dark-theme appearance still need native populated review.

### File list

FileTable shows Name, Logical size, Modified and Type in aligned columns. Rows use fine bottom rules, a neutral hover background and a blue selected background. Names use outline icons and truncation; details show the full path. The selected item can be revealed in Windows Explorer or its path copied. List pagination, scrolling and keyboard behavior still need populated native verification.

### History

Save snapshot uses the existing primary button. Stored bytes and Compact align at the toolbar's opposite end. The status panel has a 240px minimum height. Loading shows "Loading…"; a failed history request shows "History unavailable" and Retry. Both use a polite live region, hide stored bytes and disable Compact. Only the ready state can show the history icon and "No snapshots yet". These states add no explanatory paragraph.

Snapshot rows show the root path, capture date, optional Partial label and right-aligned logical size. Paths truncate; dates use secondary metadata text. Open is a secondary action. Delete exposes inline Cancel and Delete snapshot controls, with the existing error color on the confirmation action. Before and After selects limit comparisons to compatible roots; Compare stays disabled for identical snapshots. Pagination uses the existing compact buttons.

Opening delete confirmation moves keyboard focus to Cancel. Cancelling restores focus to the row's delete trigger. A failed deletion keeps confirmation open and returns focus to Cancel. After successful deletion, focus moves to Save snapshot when enabled, otherwise to the Snapshot history region. This focus routing is recorded from source and remains unverified in native interaction.

Recorded size appears only when more than one compatible saved snapshot exists. The line uses actual logical-size values from the current snapshot page. Comparison rows show a path, an observation/change label and an aligned signed delta; partial coverage remains explicit. These populated states are documented from source, not native interaction verification.

### Developer storage

Developer storage has a measured category bar and legend above a flat list/details split. The native Dependencies, Build and Cache totals cover all classified directories after nested candidates are excluded, before 50-row paging. No illustrative Other category is invented. The 340px detail column appears at 1200px and above; smaller windows stack it below the list. Selecting a row displays its indexed path, tool, sizes and file count. Open folder uses the existing guarded Explorer handler. Partial status sits outside the truncated path; unknown allocation stays Unknown. Docker remains mounted in the detail column through loading and paging, retains its independent read-only analysis and reports unknown/overlapping sizes. Its bars compare known category sizes with the largest known category, without implying additive totals. Loading, Retry and empty states keep short labels. List paging restores focus without moving it from Docker controls. Category filtering and alternate list ordering remain pending. Native visual acceptance belongs to the user.

### Manual selection preview

Cleanup uses a compact rule select for User temporary files, Old files and Project temporary files. It shows the existing age/extension criteria, candidate totals, native checkboxes and bounded pages. The user-temp rule offers Scan user temp; the other rules offer Rescan. Not indexed and Partial distinguish unavailable or incomplete coverage. Changing the rule clears selection. All selections open the shared preview without another explanatory panel.

Duplicate review uses the same preview and lists retained paths under Keep. Native checkboxes begin empty; group selection cannot consume its last copy. A compact Cancel action is available during fresh content verification. The advanced permanent action uses the same irreversible wording and typed DELETE confirmation as manual selections; the backend holds verified copies throughout removal.

File checkboxes share the name cell and appear only for complete regular files in a current scan. A compact footer reports selection count, Review selection and Clear. The native dialog presents full paths with wrapping, right-aligned Logical and Potential values and necessary hard-link labels. Close and Escape dismiss it. No files are preselected and no descriptive paragraph appears in the product. Compiler and source checks cover this addition; populated native visual acceptance remains with the user.

### Index management

Settings includes a Scan mode select with Automatic and Directory enumeration options. The saved choice applies to the next scan, and the control is disabled while an operation runs. The engine's measured method remains visible in existing progress rather than adding explanatory paragraphs to the form.

The Scanning group includes a compact Concurrency limit select with numeric values 1–8. It persists locally, defaults to 1 and is disabled during operations. No extra explanatory paragraph is added to the interface.

Settings adds an Index heading and Clear current analysis button. The action clears the loaded view after the native command succeeds and is disabled during operations or without a loaded path. It leaves saved history and local preferences intact. Engine errors use the existing notice rather than another confirmation dialog or explanatory panel.

### Search

Ctrl+K opens the root Explorer search after clearing the previous view's extension/date filters. It waits for the field to become enabled before focusing and selecting text. Active analysis and cleanup/issue dialogs block this navigation. Typing a nonempty search in Overview opens Explorer results for the existing scope. No additional interface copy is introduced.

### Large-item filters

Large files adds an Items selector beside its existing filters, with Files and Folders choices. It defaults to Files, and Reset filters restores that choice. The same virtual list and details panel display folders without another explanatory paragraph or management action. Folder rows open their indexed scope through the existing navigation handler.

### Recent paths

The toolbar shows a 170px Recent selector only when completed scans have saved paths. The native select exposes full paths in its options and resets after an explicit choice, allowing the same path to be selected again. It is disabled during operations and in the browser preview. The existing wrapping toolbar contains it without widening the document. Settings provides a compact clear action. Stored routes do not introduce another navigation panel or explanatory paragraphs.

### Cleanup activity

Cleanup activity uses a flat divided surface, compact heading and semantic theme tokens. A fixed-layout table wraps full paths and result messages rather than widening the window; its 420px scrolling body contains up to 50 rows. File, Result and Started are the only column labels. Refresh returns to the latest page; Previous and Next preserve keyboard access by restoring focus to the region after loading. Unresolved and Unknown outcomes remain visibly unconfirmed. Saved rows provide no file actions. Native visual acceptance remains with the user.

### Notices and scan issues

Preview, incomplete coverage and engine errors appear as compact bordered messages. Scan progress exposes measured counts and cancellation state. The access-issue dialog has a constrained scrolling body and an explicit close action. The browser preview explains that real drive detection and scans require the Windows app, and disables those actions.

## Do's and Don'ts

### Do:

- Do use the semantic theme colors and the existing compact text hierarchy.
- Do preserve the supplied logo geometry and its intentional pale circular backing in dark navigation.
- Do keep measured sizes, paths and incomplete coverage visible beside storage graphics.
- Do provide a list route for the Canvas map and visible keyboard focus on controls.
- Do honor both the saved motion preference and the system reduced-motion preference.
- Do keep History copy to action labels, recorded values and necessary coverage or empty-state messages.

### Don't:

- Don't invent populated charts or historical measurements for an empty scan.
- Don't change a map color into a semantic file-type promise without updating the implementation.
- Don't treat browser-only finish approval as verification of populated native or IPC workflows.
- Don't add another full sidebar when navigation labels can expand in the existing rail.

Independently generated file-type colors, virtual-list geometry and dialog accessibility behavior remain implementation details to assess in populated native views. The shared primary hover color is recorded as shipped, not as proof of complete dark-theme interaction review. No craft-floor defect was reported by the supplied mechanical scan; this record does not extend that result beyond its checked scope.

The treemap uses the reference's blue, navy, teal, orange, ochre and gray palette in both themes. Each fill has an explicit white or dark ink pair; the computed minimum contrast is 4.746:1. Labels and interaction outlines use the same paired ink. Selection requests a four-pixel stroke and hover two pixels, so their distinction also uses thickness. Strokes are clipped to each filled rectangle. These source-color calculations do not establish complete accessibility or native rendering acceptance.

The nine images in `interface/` are the user-pinned direction. Their sample data is illustrative and never shipped as measurements. The shared shell and both Overview compositions follow them. Explorer and Large files now share a flat list with a 300px details column above 1200px, stacked details below that width, and a viewport-filling virtual list. Their filters, selection, keyboard navigation and native actions remain connected. Details show measured sizes, date and item kind; folders retain file counts. Folder details show the five largest directly indexed children and their complete child count. Child buttons open indexed folders or select file details, retaining query cancellation and incomplete coverage labels. Large files adds indexed paths below names and size bars normalized to the largest item in the complete filtered result. That scale is computed before paging and stays stable across pages and sort choices. Native path construction is limited to the returned page and shares cancellation and the index read guard. Rows retain the existing 36px virtual geometry; populated visual acceptance remains manual. Cleanup now follows its rule-choice row, filename/path list and measured selection sidebar, stacking below 1200px. The sidebar preserves up to 500 selected rows across pages and resets with the rule or index revision. Review sends only IDs to the existing native validation; displayed logical totals are not reclaimable-byte promises. Recycle Bin uses an inline measured summary, and activity uses flat divided rows. Cleanup rows now show indexed modification dates, with unavailable timestamps kept explicit. Preview timestamps come from freshly verified filesystem metadata. The widened table remains inside its horizontal scrolling region. The header checkbox selects or clears only the displayed page, with an indeterminate state for partial selection. Other pages remain selected. The 500-file limit rejects an overflowing page atomically and leaves the accepted selection unchanged. Review remains explicit. Developer storage now has its measured category bar, list/details split and persistent Docker section; category filtering and size/name ordering apply before pagination. Category totals remain global while the list count follows its filter. Controls remain mounted during loading, and changing a filter resets its page and retry offset. History separates captures/trend and comparison into wide columns, stacking below 1200px, with actual entry counts and before/after sizes. Comparison category summaries now show complete regular-file totals rather than totals from the visible page. Added/Removed use logical bytes and file counts; Changed uses the signed net difference for regular files. Directory rollups are excluded. Partial newly observed/unobserved files stay separate from confirmed additions/deletions. History chart axes show whole-byte ticks and real capture dates, with measured time spacing, point markers and bounded size labels. Coincident timestamps share one position; flat series retain a finite scale. The responsive plot uses its actual width so chart text does not shrink with a fixed view box. Duplicates now uses a paged groups/files split with indexed representative names and member dates, explicit loading/retry and a bottom selection-review bar. Potential allocation remains an estimate; retained-copy authority stays in the verified cleanup preview. Settings now follows its divided form and visible theme choices. The map now draws a second level inside sufficiently large folder blocks. Overview returns at most 12 folder branches with eight logical-size-ranked direct children each, collected in one cancellable index pass with bounded retained candidates. Child weights follow the selected logical/allocated metric. Other is the measured parent remainder; inconsistent partial totals or small blocks retain the parent-only view. Hit testing chooses nested items before their enclosing folder, and opening a folder loads its own measured scope. Native rendering and whole-app performance acceptance remain manual. No browser or native visual automation was used.

The file-type summary displays eight ranked groups and a neutral Other segment. Segment weights use measured logical bytes; Other includes every remaining regular-file group, including groups omitted from the bounded native response. Its legend shows combined bytes and file count without offering an extension-filter action. Empty files contribute to counts but add no byte weight. This behavior is source- and fixture-verified; native presentation remains for manual acceptance.

History captures include a 16px comparison checkbox. A pair contains at most two captures with the same native root key; checkbox choices order them by capture time and stable ID. The existing Before/After controls remain available for explicit direction. Selecting or clearing captures invalidates pending comparison responses; Compare stays disabled until two distinct captures are chosen. Populated visual and keyboard acceptance remain manual.
