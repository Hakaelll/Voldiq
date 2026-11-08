---
name: Voldiq
description: Compact Windows storage analysis with measured data and a white workspace.
colors:
  accent: "#2963a6"
  accent-fill: "#2963a6"
  accent-hover: "#173784"
  selected: "#e6edf5"
  canvas: "#f7f8f9"
  surface: "#f2f4f6"
  paper: "#eaedf0"
  border: "#dfe3e8"
  strong: "#7b8692"
  text: "#262d36"
  secondary: "#525e6b"
  muted: "#586575"
  map-surface: "#eff2f5"
  dark-map-surface: "#111419"
  error: "#a32424"
  dark-canvas: "#0c0e11"
  dark-surface: "#111318"
  dark-paper: "#191c22"
  dark-border: "#2a2e35"
  dark-strong: "#707070"
  dark-text: "#ededed"
  dark-secondary: "#c2c2c2"
  dark-muted: "#ababab"
  dark-accent: "#8caff0"
  dark-accent-fill: "#345787"
  dark-selected: "#253347"
  dark-error: "#f29797"
  map-blue: "#3272d1"
  map-violet: "#7a63c6"
  map-teal: "#219b8c"
  map-orange: "#e36a4a"
  map-ochre: "#e8aa35"
  map-pink: "#b76ac5"
  map-cyan: "#4aa3d5"
  map-slate: "#8a96a8"
  map-unknown: "#616e81"
typography:
  headline:
    fontFamily: "Segoe UI Variable, Segoe UI, sans-serif"
    fontSize: "26px"
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
  data:
    fontFamily: "Segoe UI Variable, Segoe UI, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.5
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
  radius: "6px"
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

The unscanned Overview leads with the drive scan and folder picker, followed by up to three recent paths. A divider separates the scan actions from the selected drive's capacity chart and measured bytes. The sections stack below 850px in the same reading order. Missing measurements stay unavailable and scanning starts on request.

Storage cost is an optional Overview section. The dated tariff table aligns the current and scenario monthly charges. An explicit action can capture the indexed folder's logical size as a billing assumption. The editor opens inline and tariffs stay in the current Overview session.

Libraries shares the Overview section navigation. Games and Media use a flat list with the application and measured subcategories. The shared manual folder list reports separate indexed totals and explains potential overlap. Unknown media folders receive size information only.

The library list identifies recorded snapshot evidence near its measured totals. A snapshot without a capture offers the next step without inventing sizes.

Overview groups Drive health under Space & drive diagnostics. Check drive is explicit. The report pairs its checked time and scan root with a short row of device counters or the Windows access or support state for each mapped disk.

The Space account explains where to check a live file. In the file inspector, Check file use runs on request alongside the preview and download source actions. Reported applications appear in a compact list with their process IDs. Empty, incomplete and cancelled checks have distinct messages. The scope is one file name at the time of the check.

Overview, Cleanup, History and the Explorer list share text section controls with an underline for the active view. Explorer offers Map, List and Grid at this level. Map styles are chosen inside the map toolbar. The workspace toolbar shows drive and search controls on analysis views and the drive picker in Windows tools. Cleanup opens directly to candidates, with Recycle Bin and Windows Storage in Windows tools. History puts the measured trend before its snapshot rows and groups storage maintenance under Manage snapshots.

Cleanup ranks files under a single category picker. The table leads with each file's name, path and modification date, followed by aligned sizes and review priority. A priority opens its evidence beneath the file row. Weight controls stay inside a disclosure and reordering preserves explicit selections.

Explorer places the path and coverage status next to the page title. The scan uses one compact status row. Idle Live, Folder budget and list filters use compact controls. Expand chart uses the same map surface with more vertical room and exits with Escape. The Treemap fits the available aspect ratio. Labels that fit within a tile use its own foreground color. Theme selection lives in Settings.

Chart types share a visible segmented radio group with a checked surface and visible keyboard focus. Export and Expand chart sit together at the far end. The lower row separates chart gestures from Map settings and Files. The Files disclosure contains List view, Grid view, map items and Inspector where available. Choosing an action collapses that disclosure. The arrangement follows the visual hierarchy of the [TreeMap map toolbar](https://github.com/Prithvi-Web/TreeMap-Disk-Visualizer/blob/main/src/ui/markup/015-view-treemap.html).

The live file inspector groups previews and download provenance under Inspect file. Reading starts with an explicit action. Text stays plain, thumbnails are static, and metadata remains visible when content is unsupported. Download provenance shows the recorded domain first. A separate button reveals the full URL as selectable text and closing the disclosure clears it.

Archive previews use a paged list of read-only names, entry kinds and declared file sizes. The inspector names any coverage limit and counts skipped records. Archive entries have no file actions or links.

Populated maps show each sufficiently wide root region's percentage of the rendered measured weights, using the selected size metric. Nested children are excluded from this denominator to avoid counting the same bytes twice. The selection caption pairs its path and selected size with its indexed file count. File-type legends use responsive columns with aligned swatches and measured size/count labels.

The populated Overview follows the supplied storage-dashboard references through composition and data density. A 12-column layout pairs the system/indexed summary (four columns) with file types (eight), followed by the dominant storage map (eight) and ten largest items (four). Sections use a common workspace surface and separators. The 112px measured volume ring stays separate from indexed logical size. File types use a 176px donut and keyboard-accessible extension filters with bytes, file counts and percentages; Other retains the actual remainder. Each leading extension uses a distinct rank color shared with its legend. Total bytes and file counts also appear as ordinary accessible text. Space accounting and recovery copies remain available in a full-width disclosure below the primary analysis. At 1180px the summary/type panels become full-width; at 1000px the map/ranking stack; at 840px the summary and type legend stack. No ranking is hidden. Allocation captions retain Unknown/known-partial formatting. Native rendering, keyboard and scaling acceptance remain manual because Computer Use is excluded.

Explorer rows now pair their logical sizes with compact bars scaled to the largest item in the complete filtered result, retaining the virtual row geometry. Folder details show a 48px composition strip for the five measured direct children and the parent remainder. Child colors match the actionable list below. The strip is omitted when child totals exceed the indexed parent size; it never fills missing measurements with sample values.

Explorer and Large files expose ordering on their Name, Logical size and Modified column headers. The active column uses stronger text, a directional chevron and `aria-sort`. Name is ascending, size and modification date descending. Header actions and the Sort selector share one typed query state; changing order resets the virtual list without changing its row geometry.

Developer storage uses a 44px measured category strip with bytes, percentages and directory counts below. Its category legends are filter buttons with pressed states; choosing a category twice or choosing All categories clears that filter. The legend and reset/order controls remain mounted through loading, while interaction is disabled. Category totals continue to describe the complete deduplicated result rather than the currently filtered page.

Duplicate group rows include compact meters relative to the report's total potential allocation. The active group's copy-balance strip compares its unselected and selected copy counts across member pages. Each member identifies Keep or Selected for review, and selected rows use the shared selection surface. These are selection states, not deletion results or verified retained-file authority; the native review remains mandatory.

Duplicates has separate Exact copies and Similar images sections. Similar images explains the visual distance and the current folder scope above a ranked pair list. A selected pair opens two adjacent previews with shared zoom and independent scrolling. Each preview shows the path, oriented dimensions, logical size and recorded modification date. Checkboxes start empty and send the chosen files to the manual review tray. Visual distance grants no exact-duplicate authority.

History compares logical file changes with three bars on a shared signed scale around a central zero line. Added bytes extend right, removed bytes left, and Changed uses its complete signed net difference. Values and file counts remain readable beside each bar, including zero-byte changes. Newly observed and unobserved coverage stay separate and do not affect this scale. No extra chart runtime is loaded.

**Creative North Star: "The Windows storage workspace"**

Voldiq puts paths, file sizes and scan coverage in a compact desktop workspace. The built interface follows the user-selected white Windows direction and supplied Palantir, Linear and Dub references. Fine dividers organize the rail, toolbar and content without decorative elevation. Segoe UI keeps the controls consistent with Windows expectations.

The supplied stacked-storage logo is the identity asset. Its disc-shaped top and blue segment remain intact. Light mode is the default; dark mode changes the same semantic color roles. Both themes use the original raster through a centered 160% viewport crop in a bounded rounded frame, without added backing or padding.

**Key Characteristics:**

- A labeled 224px sidebar is the default. An explicit footer button switches to a 64px icon rail; the choice persists with existing appearance preferences. Hover and keyboard focus never resize the workspace.
- White content areas and fine neutral dividers support compact controls.
- Blue marks selection, focus and primary actions.
- Measured storage data determines chart area and totals.
- The original Voldiq logo remains visible in both themes.

This document records the initial built UI in `frontend/styles.css`, `frontend/App.svelte` and the component files. Finish approval covers the scored dark-logo matte and contrast corrections plus the initial browser empty and settings views. Populated native views, scan progress, IPC, the Canvas map, the virtual file list and access-issue dialog remain unassessed in a running Windows session. The mechanical detector reported no findings and the shipping raster provenance scanner reported zero missing entries; neither result proves those native workflows.

History extends this system with saved-snapshot controls and compact comparisons. Earlier desktop light, desktop dark and minimum-window dark screenshots showed its browser empty state. The subsequent loading, unavailable and deletion-focus changes were checked in source only. Saving, opening, deleting, compacting and comparing populated native snapshots remain unassessed.

## Colors

The palette uses white and neutral gray for structure, blue for interaction, and distinct category colors for the storage map. Frontmatter records exact source values. Each `dark-` token replaces its matching light role under the root dark-theme attribute.

### Primary

- **Selection blue** uses `accent` for current navigation, focus and carets; `selected` supplies the pale selection background.
- **Action blue** uses `accent-fill` behind primary button text. `accent-hover` is the actual shared hover value in both themes.

### Secondary

- **Measured map colors** share `frontend/lib/chart-colors.ts`. Type mode distinguishes folders (blue), images (gold), video (coral), audio (pink), code (teal), archives (violet), documents (cyan) and other files (slate). Remainder/unknown data stays neutral. Legacy ranked maps use the same eight-color palette. Size and age keep their existing logarithmic/date bands and use separate five-color ramps drawn from this single palette source; legends consume those exact ramps. Colors describe data categories or the selected metric, never cleanup permission or risk.

### Neutral

- **White canvas** uses `canvas` for ordinary controls. `panel` aliases canvas in light mode and surface in dark mode; `workspace` aliases surface in light mode and canvas in dark mode, separating the analysis panels from their background.
- **Quiet containers** use `surface` for the rail, empty state, notices and table headers. `paper` supports hover and inset backgrounds.
- **Fine structure** uses `border` for dividers and `strong` for fields, tooltips and dialog outlines.
- **Reading hierarchy** uses `text`, `secondary` and `muted` for primary content, explanation and metadata.
- **Error text** uses `error` within the same neutral notice container.

**The Semantic Theme Rule.** Use the existing semantic custom properties for new controls so theme changes update their text, background, border and focus together.

## Typography

The UI uses Segoe UI Variable with Segoe UI and sans-serif fallbacks. It has no separate display face. Canvas text uses Segoe UI explicitly. Use the frontmatter roles as the recorded hierarchy; paragraph body text has the body line height, while controls keep their native inherited line height.

Page headings use a compact 24px headline; panel and settings headings use title, paragraphs and controls use body, dense file rows use label, and footer annotations use metadata. Indexed logical size uses a 29px value. The welcome heading is 28px and the wordmark 23px (24px at the compact breakpoint). The existing Segoe family and numeric alignment remain authoritative.

**The Measured Number Rule.** Keep storage sizes and file counts aligned with tabular numerals. Right-align the file list's logical-size column; allow names and paths to wrap or truncate according to their container.

## Layout

The desktop shell reserves a stable 224px labeled sidebar or a manually selected 64px icon rail at every desktop breakpoint. Navigation owns its overflow, preserving access to all pages, settings, the explicit collapse control and a light/dark theme shortcut. The command palette has a persistent sidebar search button and retains Ctrl+K. Its button opens commands; the independent toolbar search still searches the current analysis. The toolbar wraps within a 64px minimum height, with 24px desktop content insets. Settings keeps its existing labeled controls, native appearance radios and motion checkbox; labels stack at 840px. No hover expansion or overlay obscures analysis content.

The populated overview follows `interface/01-overview-root.png` and `02-overview-folder.png`: a 248px summary column with measured capacity, index totals and largest items, followed by a dominant map and file-type legend. The summary narrows to 210px below 1100px; below 840px the sections stack without hiding largest items. Map height scales from 360px to 600px. The capacity strip remains only in the unscanned overview. Other data views use their shared header without an extra capacity strip. The file list retains its 36px logical rows and existing bounded virtualization.

History uses a 12px control gap and 18px spacing below its toolbar. Snapshot rows have a 68px minimum height, medium vertical padding and panel-gap horizontal padding. Before and After selects wrap with a 180px minimum label width. Its recorded-size graphic is 110px high and uses the existing accent color.

At 1100px and below, main padding becomes 20px and toolbar spacing tightens. The chosen sidebar width stays stable. At 840px the toolbar search occupies its own row and settings labels stack. Dashboard and welcome breakpoints are described above; long paths are bounded or wrapped, and totals retain explicit labels. Reduced-motion preferences continue to disable existing button transitions. Desktop resizing, Windows scaling and native focus remain manual acceptance checks.

**The One Rail Rule.** Expand labels within the existing navigation rail. Keep the workspace as the adjacent main area.

## Elevation & Depth

The built interface uses no box shadows. One-pixel borders and adjacent neutral backgrounds separate panels, fields and navigation. The access-issue dialog uses a fixed black backdrop at 25% opacity and a stronger border. Its depth comes from occlusion and the backdrop.

Buttons transition their background over the root motion duration of 140ms. Rail width uses the same duration with ease-out. The system reduced-motion preference and the saved Reduce motion setting suppress transitions and animations.

**The Border Depth Rule.** Use the existing border and background roles to organize content. Keep panel depth consistent with the flat built containers.

## Shapes

Controls, panels, notices and selection backgrounds use the radius token. Smaller chart insets and keyboard hints use the inset radius. The dialog uses the dialog radius. Table rows use square corners and bottom dividers.

Icons are inline outline SVGs, generally 18px, with a 24-unit viewbox, 1.6-unit current-color stroke and round caps and joins. The shared BrandMark crops the original 256px artwork identically in navigation and welcome states. Its frame follows the requested size without an added dark-theme circle or inset.

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

Projects and Git repositories share the local section navigation. Git uses a flat size table ordered by observed total. Partial coverage and unresolved stores appear beside the results. Repository paths open the matching indexed folder in Explorer.

Developer storage has a measured category bar and legend above a flat list/details split. The native Dependencies, Build and Cache totals cover all classified directories after nested candidates are excluded, before 50-row paging. No illustrative Other category is invented. The 340px detail column appears at 1200px and above; smaller windows stack it below the list. Selecting a row displays its indexed path, tool, sizes and file count. Open folder uses the existing guarded Explorer handler. Partial status sits outside the truncated path; unknown allocation stays Unknown. Docker remains mounted in the detail column through loading and paging, retains its independent read-only analysis and reports unknown/overlapping sizes. Its bars compare known category sizes with the largest known category, without implying additive totals. Loading, Retry and empty states keep short labels. List paging restores focus without moving it from Docker controls. Category filtering and alternate list ordering remain pending. Native visual acceptance belongs to the user.

### Manual selection preview

Cleanup uses compact icon-and-label buttons for User temporary files, Old files, Project temporary files and Browser HTTP cache. A filled selection surface and pressed state identify the active rule. It shows the existing age/extension criteria, candidate totals, native checkboxes and bounded pages. The user-temp rule offers Scan user temp; the other rules offer Rescan. Not indexed and Partial distinguish unavailable or incomplete coverage. Changing the rule clears selection. The selected-file workspace uses the surface tone instead of another vertical separator. Each selected file has a measured logical-byte share bar relative to the complete selection; empty files retain their names and sizes without a filled bar. All selections open the shared preview without another explanatory panel.

The user-requested Taste Skill source review informed this composition. Its marketing-page rules are adapted to the desktop task: the supplied palette, table separators and data graphics remain authoritative. The source linter flagged hairlines, the measured capacity ring and interaction shadows; those findings are not a visual score. No random marketing direction, photographs, decorative animation or automated browser inspection was introduced. Native visual acceptance remains manual.

Duplicate review uses the same preview and lists retained paths under Keep. Native checkboxes begin empty; group selection cannot consume its last copy. A compact Cancel action is available during fresh content verification. The advanced permanent action uses the same irreversible wording and typed DELETE confirmation as manual selections; the backend holds verified copies throughout removal.

File checkboxes share the name cell and appear only for complete regular files in a current scan. A compact footer reports selection count, Review selection and Clear. The native dialog presents full paths with wrapping, right-aligned Logical and Potential values and necessary hard-link labels. Close and Escape dismiss it. No files are preselected and no descriptive paragraph appears in the product. Compiler and source checks cover this addition; populated native visual acceptance remains with the user.

### Index management

Settings includes a Scan mode select with Automatic and Directory enumeration options. The saved choice applies to the next scan, and the control is disabled while an operation runs. The engine's measured method remains visible in existing progress rather than adding explanatory paragraphs to the form.

The Scanning group includes a compact Concurrency limit select with numeric values 1–8. It persists locally, defaults to 1 and is disabled during operations. No extra explanatory paragraph is added to the interface.

Settings adds an Index heading and Clear current analysis button. The action clears the loaded view after the native command succeeds and is disabled during operations or without a loaded path. It leaves saved history and local preferences intact. Engine errors use the existing notice rather than another confirmation dialog or explanatory panel.

### Search

Ctrl+K opens the root Explorer search after clearing the previous view's extension/date filters. It waits for the field to become enabled before focusing and selecting text. Active analysis and cleanup/issue dialogs block this navigation. Typing a nonempty search in Overview opens Explorer results for the existing scope. No additional interface copy is introduced.

The Advanced button opens a compact nonmodal query editor below the toolbar. It uses the existing field, button, focus, border and surface tokens. A draft receives structured syntax inspection from the core after a short debounce; it does not replace applied results while typing. Syntax checked is distinct from permission or execution approval. Apply requires the exact validated draft, current query object and current analysis. Late replies, repeated text from an older request and context changes cannot enable it. Cancel/Escape leave the applied query intact and restore focus when the editor still owns it. Leaving indexed views or adopting another analysis dismisses the editor. Applied expressions remain visible in a compact bar with Clear expression.

Apply preserves the current map/list/grid representation, root, byte metric, color and inspection/cleanup-review sets. Other visible filters continue to intersect with the expression. Native syntax-error byte offsets become readable text positions. The bounded validation channel is separate from result queries; a pending syntax check cannot cancel the displayed list. No modal backdrop, new font, animation or decorative color is introduced.

Search highlighting in all five maps uses scoped core matches, including matching folder groups and entries beyond the drawn frontier. Matching a folder highlights its represented contents; tooltips explain that context. Exact remainder counts preserve matches outside loaded branches. Areas keep the selected byte metric and existing cloud projection. The footer distinguishes indexed matches from highlighted regions, and an item-list route covers matches without a drawable region. Loading and failure suppress stale highlights and expose status/retry rather than a zero-match claim. Native rendering, focus/keyboard and Windows scaling remain manual acceptance checks.

Describe a search is a collapsed native disclosure inside the Advanced editor. It reuses existing fields, buttons, Segoe UI, spacing and semantic colors. Interpret shows a local rule-based proposal, its condition count and the exact unparsed text. A partial interpretation requires the explicit Use only the interpreted conditions checkbox before Use as editable draft. The ordinary editable expression and syntax-checked Apply remain separate. No query runs from interpretation alone. Changing the description, draft, filters or analysis invalidates the proposal and its partial acceptance. Media categories describe filename extensions, not inspected content. Explicit UTC dates replace ambiguous relative ages; unknown intent remains visible.

### Saved workspaces and commands

Saved opens a compact two-column panel for named views and favorite folders. It saves all applied filters, page/representation, metric, depth, color and an explicit Current folder when loaded or Whole current analysis scope. It stores no transient analysis node IDs or list pagination. Load restores the view within the current analysis and keeps inspection and cleanup-review sets. Create, rename/change scope, replace and remove are distinct actions; secondary management controls stay under a disclosure. Errors expose Reload. Saving preferences is disabled while an operation owns the shared lease. A favorite identifies an observed local directory; Scan is an explicit new scan with native identity revalidation, using the existing scanner. Missing/replaced folders require choosing them again.

Commands and Ctrl+K open a native dialog with one search field and bounded matching rows. The palette includes existing pages, current-analysis file search, saved views, the folder picker and explicit favorite scans. It filters labels/details, skips disabled commands, supports arrows/Home/End/Enter/Escape and uses the native modal focus boundary. It does not open over an existing modal review. Close returns focus when still owned; commands retain their ordinary index/operation/revision checks. No shell or destructive shortcut is added. Literal file search remains separate in the toolbar.

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
- Do preserve the supplied logo geometry and the shared centered crop in both themes.
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

The file-type summary displays eight ranked groups and a neutral Other segment. The 44px strip makes each named segment a filter button; segments with at least 12% of bytes also show their extension. Paired palette inks preserve label and keyboard-focus contrast in both themes. The persistent legend keeps tiny segments and zero-byte groups accessible. Segment weights use measured logical bytes; Other includes every remaining regular-file group, including groups omitted from the bounded native response. Its legend shows combined bytes and file count without offering an extension-filter action. Empty files contribute to counts but add no byte weight. Largest-item rows add a thin bar showing logical bytes relative to the indexed directory, using the same color as their map swatch. Selection retains its existing action and exposes its pressed state. Native presentation remains for manual acceptance.

History captures include a 16px comparison checkbox. A pair contains at most two captures with the same native root key; checkbox choices order them by capture time and stable ID. The existing Before/After controls remain available for explicit direction. Selecting or clearing captures invalidates pending comparison responses; Compare stays disabled until two distinct captures are chosen. Populated visual and keyboard acceptance remain manual.


Folder navigation in the map uses a finite 160 ms ease-out transition when a measured source or destination rectangle is visible. Pointer entry expands from that rectangle; returning to the parent contracts into it. The previous and current bounded layouts crossfade without bitmap capture. Keyboard navigation, ordinary data refreshes, metric changes, resizing, first render and movement-reduced settings stay immediate. Input interrupts outstanding frames before hit testing, and hidden or unmounted maps cancel their frame. Native presentation and frame-rate acceptance remain manual.

## Skyline, version 0.2.0

Skyline extends the existing storage explorer with a Canvas 2D architectural view. This section supersedes the earlier rank-colored nested-map description. Treemap and Skyline consume one byte-conserving frontier in fixed coordinates. File-category colors are amber, terracotta, olive, teal, violet and neutral; size and age use sequential teal scales. The default height is descendant modification age with explicit coverage, never last access. The small rooftop equipment and facade windows are deterministic decoration, clipped to their building and removed at small pixel sizes.

The shared light surfaces now use warm whites and graphite text, while the existing blue accent and supplied artwork retain the product identity. Dark tokens stay intact. Controls share a six-pixel radius. Storage representations use a compact segmented control; camera controls remain on the viewport. Page headings use a local 24px size and the visual explorer tightens heading, scan-status and breadcrumb spacing. The rail remains integrated and collapsible. The existing 300px inspector becomes an overlay below1200px, and the paged file list remains available below the map or as a full view. No ambient animation is introduced.

The canonical implementation is frontend/styles.css, frontend/components/StorageMap.svelte and frontend/lib/storage-map.ts. Technical details and evidence are documented in docs/SKYLINE.md; that file does not define a second visual system.

## Historical visual comparison

Compare snapshots expands to a full-width workspace while retaining the capture selectors and paged change details. Back to captures restores the selection area. Two historical treemaps use the existing layout and category palette with one byte-to-area scale. Neutral plot area is explicitly separate from volume free space. Labels retain each capture's save date and independently known measurement date. Recorded absolute and relative differences remain distinct from a claim of physical growth.

The divider follows the [WAI-ARIA slider pattern](https://www.w3.org/WAI/ARIA/apg/patterns/slider/), with a named value, arrow steps, Home/End and larger Page Up/Down steps. Pointer capture keeps a drag attached to its handle. Root/Up, a paged union of recorded folders and a ranked-entry table provide coordinated navigation without turning persisted path IDs into file-action IDs. Each map retains at most 160 entries and an explicit remainder; the folder picker includes omitted and zero-byte folders in pages of 100. Loading and failure replace stale geometry, with reload and root recovery. No new animation, assets or visual tokens are introduced. Populated rendering, keyboard/focus, assistive technology, scaling and device performance remain manual acceptance.

## Folder budgets and measured estimates

The indexed workspace has one compact, expandable Folder budget panel above its main content. It follows the existing bordered panels, spacing, typography and theme tokens. Its summary shows logical path bytes, the saved limit, a bounded progress indicator and a written status. Partial or provisional coverage says At least; a proven excess remains visible without implying known remaining capacity. Limits belong to a verified native folder identity. Saving and removing a limit are explicit human actions with revision checks; historical views remain read-only.

Opening the panel requests its detailed analysis. Forecasts use distinct, compatible measurement days rather than snapshot save times. Unavailable, stale, partial, variable and non-growing histories have specific messages. An estimate shows a calendar date, an observed-rate range and its measurement basis; it does not predict volume free space. Image-file equivalents disclose the first bounded name-ordered sample of actual indexed image-extension matches and their mean logical size. They do not identify content or assume a typical photo size. Native rendering, keyboard/focus and Windows scaling acceptance remain manual.

## Radial storage views

Sunburst and Circles share the bounded hierarchy, byte metric, scope, selection, details, note previews and camera controls with Treemap. Sunburst uses sector angle for bytes, neutral bands for parents and colored terminal sectors that reach the outer edge. Circles uses one square-root byte scale across all depths; ancestor outlines enclose descendants without counting their bytes again. Its legend discloses the measured packing fraction and distinguishes gaps from free disk space. Structural shapes remain selectable for folder navigation. Empty or unavailable geometry reports no invented packing percentage. Representation controls wrap within narrow panels, and List remains the keyboard-accessible equivalent. Geometry fixtures prove conservation, proportional area, containment, nonoverlap and picking; populated rendering, interaction and Windows scaling acceptance remain manual.

Depth uses one application-owned value from one through six across Overview, Explorer and all map modes. The selector limits the loaded hierarchy and preserves the same byte total; readability limits may keep small groups collapsed earlier. Selection and details remain available when a selected descendant no longer has a drawn shape, with an explicit status message. Changing depth does not request a scan or replace the loaded tree.

## Explorer grid and table

Grid reuses the table's paged data, scope, filters, current index, inspection and cleanup checks. Tiles are flat divided cells using the existing surfaces, typography, swatches and file/folder icons. Their equal area is explicit; indexed byte values carry size. A visible outline and current-item semantics identify inspection, while checkboxes select files for cleanup review. One through eight columns follow the available width. Virtual positions, page requests and keyboard movement share one geometry model with the table. Column or representation changes reposition from the first visible item. Loaded filenames, modification dates and unknown states remain plain text; no content preview is loaded.

Explorer's Size control shares the map's logical/unique-allocation preference. Largest-first ordering follows the chosen metric through the core. Unknown allocation is labelled Unknown; incomplete positive evidence is labelled known and partial. Logical size bars retain their global logical scale and are omitted for allocation. Large files retains its explicit logical-size view. Loading and unavailable results remain distinct from an empty query, with a persistent retry action after failure. Native rendering, focus, scaling and measured device performance remain manual checks.

## Weighted Voronoi

Voronoi uses the same depth-limited frontier, current scope, snapshot, area metric, colors, inspection and camera controls. It draws a flat weighted power diagram; parent navigation remains available through the existing hierarchy and list routes. A damped Newton solver adjusts cell areas in a disposable worker. Small entries beyond the 120-cell detail budget share one neutral aggregate, retaining their memberships and indexed bytes. Its reserved synthetic identity opens the complete item list and never reaches a native command.

The legend identifies approximation, grouped frontier count, area discrepancy and worst relative cell error. Area discrepancy is half the sum of absolute differences between area shares and byte shares; worst error is relative to the intended area of one cell. Tooltips also show signed cell error. Accepted diagrams require discrepancy at most 0.2% and worst-cell error at most 5%; the solver aims more tightly. Unsupported size ranges, exhausted precision or unavailable workers offer Treemap/List. Unknown quality on empty data is not shown as measured zero. Filling, label fitting and picking share the returned convex polygons; labels render only when their box fits within a cell. Source geometry and worker-control fixtures do not establish populated native visual, focus, scaling or device-performance acceptance.

## Agent connection settings

Settings extends the existing divided form with Agents and permissions. Authorized connections use compact rows with plain-text paths, last use, expandable scope/limits and explicit rotation/revocation controls. Creation stays in the same form rhythm, using the existing Segoe UI typography, surfaces, borders, blue primary action and six-pixel radius. Credentials remain hidden until an explicit reveal, can be copied by an explicit action and are not persisted in frontend preferences. Creation and rotation use a separate native review dialog. No new sidebar or visual token system is introduced. Source diagnostics and frontend/native compilation do not establish native visual, focus or Windows scaling acceptance; those checks remain manual under the user's no-computer-use instruction.

## Shared inspection selection

Overview, every map and Explorer table/grid share a bounded inspection selection within the exact current analysis. A plain click replaces it, Ctrl/Meta+click toggles an item and Shift+click adds one. Focused item buttons also accept Ctrl/Meta+Space. A wrapping, compact strip reports the selected count, modifier hints and Clear inspection. Escape dismisses a folder-note preview first, then clears inspection. Native visual/keyboard/focus/scaling acceptance remains manual.

Folder selection outlines its represented frontier descendants through explicit parent relations, including when navigating inside that folder. Items outside the visible frontier remain selected; the count spans the current analysis. No overlapping folder-byte totals are summed. Table/grid and largest-item rows add an inset blue marker and expose their inspection state to assistive technology. Primary details remain singular. Root, depth, metric, color, filter and representation changes preserve inspection within an index; adopting another analysis clears it. At most 500 distinct native entry IDs are accepted atomically. Synthetic map groups keep their existing List/Open routes.

Inspection state and the cleanup-review checkboxes use separate sets. Clearing inspection leaves cleanup review intact. The cleanup footer explicitly names its own count; only its existing Review selection action prepares a native cleanup preview. A common cleanup tray remains pending.

## Treemap lasso and lens

The existing toolbar adds Pan, Lasso and Lens 2× controls in the same pressed-button group style. Lasso draws a freehand, automatically closed outline with a contrasting double stroke and light fill. It selects frontier regions with positive area inside that outline; bounding-box overlap or a border touch alone does not select an item. Ctrl/Meta toggles and Shift adds, captured at gesture start. Its batch updates only inspection state, shares the existing 500-item limit and clears singular details after acceptance. Synthetic regions are counted as skipped and retain their separate item-list route. No native cleanup plan is prepared by this tool.

The displayed outline is sampled and simplified to at most 256 points; selection uses that exact displayed polyline, with the final pointer position included. At most 350 frontier regions enter the geometric calculation. Crossed, degenerate or unavailable outlines report an error and preserve inspection. Gestures own one pointer and the starting camera, analysis and geometry context. Escape, pointer cancellation, lost capture, focus loss, map/root/index/depth/metric/viewport changes and camera navigation cancel an unfinished gesture. A second pointer cannot replace its owner. List/grid remains the keyboard alternative for multiple selection.

Lens 2× clips a circular local viewport and invokes the existing Treemap painter with a new doubled-scale camera. It redraws shapes, labels, search and inspection outlines from cached geometry; it never enlarges a bitmap. The base camera and measured byte scale remain unchanged. A center mark identifies the pointer's original world position. Pointer movement or keyboard camera movement requests drawing; idle has no animation loop. The lens is hidden while drawing a lasso or panning, and its legend identifies local magnification. The keyboard-focus position defaults to the map center. Native rendering, gesture/focus interaction, Windows scaling and device performance remain manual checks.

## Map context actions

Right-click opens a compact context menu for the region returned by the current view's existing picker. Shift+F10 and the Menu key use the visible selected item, or the map background when no selected region is available. The menu names its target and uses the existing canvas, border, radius, typography, selection surface and focus tokens. Native entries offer Inspect, Explore folder when applicable, Show in Explorer, Copy path and Open item list. A synthetic contents region offers its containing folder and the list; root/Voronoi aggregates offer only the list. Click and menu routes share one decoder, so aggregate identities cannot become native file actions. In narrow windows, the floating details pane stays below menus and dialogs.

Opening or dismissing the menu leaves inspection and cleanup-review sets intact. Choosing an action reuses the current inspection/navigation handlers, index-checked Explorer command and clipboard flow. Copy path retains exact-path checks and ignores cancelled, superseded or stale metadata replies. It reuses already loaded details for the same item. Menu dispatch checks the captured model instance, its command membership, geometry context and analysis; the application independently checks the expected analysis. No cleanup or arbitrary shell action appears in this menu.

The menu measures its actual box and clamps it to the viewport, with internal scrolling when needed. Keyboard focus starts at the first item; arrows wrap and Home/End move to the ends. Native buttons retain Enter/Space activation. Escape and Tab close the menu and return focus to the map before normal Tab movement. Outside pointer/focus/scroll events, window blur, camera changes and map/index/geometry changes dismiss it. Old callbacks cannot dismiss a newer menu. The list route moves focus into its workspace after the view changes, with interaction ownership checks. Native rendering, placement, focus, keyboard, clipboard and Explorer acceptance remain manual.

## Observed cloud filters

One compact native select joins the existing map toolbar and list/grid filter bar. It distinguishes all observed states, placeholders, observed recall flags, no observed recall flag, not a detected placeholder and unknown state. Its selection stays application-owned across representations, folders, snapshots, metrics and colors; Reset filters restores all states. Unknown evidence is never labelled local or online-only, and no-recall flags do not prove complete local content. No new color mode, icon family or decorative badge is introduced.

Maps encode the matching indexed leaf bytes and dates, with structural folders grouping those descendants. An exact remainder retains matching entries beyond the bounded hierarchy. The legend distinguishes these filtered areas from the full summary/detail totals; the Overview caption names Full folder or Full item. The item list matches each entry's own cloud observation and searches the whole current scope when a cloud filter is active. Type labels identify file/folder placeholders; details expose the observed cloud state. Unknown allocation reads Unknown, with positive partial evidence labelled separately.

Changing cloud filters preserves inspection and cleanup-review sets. The previous geometry clears while a new projection loads, and its identity changes so pending menus, gestures and workers cannot retain it. Loading and failed/cancelled projection states remain distinct from a measured zero-byte result, with an inline Retry map action. Existing toolbar wrapping, select styles, list geometry and focus tokens are reused. Real-provider behavior, native visual/keyboard/focus acceptance and Windows scaling remain manual checks.

## Recorded history playback

History's Playback action expands the existing comparison column into one full-width root map. A compact wrapping toolbar offers Play/Pause, Previous, Next, a native speed select and Loop. A labelled native range identifies the selected capture and saved date. The root, recorded logical bytes and measurement provenance sit above the 320px map, with partial coverage in text and ranked names in a disclosure. Existing map geometry, category colors, panel borders, spacing and focus tokens are reused by playback and comparison through one renderer.

Playback covers compatible captures on the loaded history page; its caption explains how to choose another page or root. Each frame is real, with no interpolated bytes or dates. Maps share the largest recorded root scale within the selected accounting group. Legacy comparability is explicitly unverified, and neutral plot area is not free disk space. Play waits for each bounded metadata response, pauses when the document becomes hidden or an operation starts, and disposes pending timers/reads when leaving. Open full snapshot is an explicit operation. Entering Playback routes displaced focus to its first enabled control. Reload returns displaced focus only while its request remains current; a later user focus choice is preserved. Back to captures restores the existing capture-list focus route. These are source behaviors; native rendering, actual playback timing, keyboard/focus and Windows scaling remain manual acceptance.

## Reviewed snapshot retention

History adds one collapsed Snapshot retention disclosure above its existing workspace. Opening it loads the selected history group's persisted preference. A wrapping form offers All saved captures or a bounded count of the newest saves; saving the preference and reviewing removals are separate actions. The group path, saved-capture count and reload state remain visible. Existing Segoe UI typography, panel borders, spacing, form fields and danger buttons are reused.

Review shows at most 100 exact captures in a keyboard-scrollable table with capture ID, saved date, logical size and Complete/Partial coverage. The total eligible count is explicit, and later batches require a fresh review. A separate checkbox acknowledges permanent loss of those historical records before the Remove action becomes available. Source files remain on disk; Compact remains separate and no freed-space guarantee is shown. Edits, reload, disabled operation state and closing invalidate the review. Entry/reload/cancelled-action focus uses the existing owned-focus conventions. Native visual, keyboard/focus and Windows scaling acceptance remain manual.
## Calendar and observed diary

History adds Calendar and Diary to the existing Compare/Playback secondary controls. Each uses the full workspace width while keeping the existing sidebar, theme tokens and snapshot selection. Reload captures provides recovery when another window retires history. The capture selector is explicitly limited to the loaded history page; the underlying month/day reads cover the complete selected capture, and the diary can page older observations within its retained group.

Calendar uses a labelled native UTC month input, month navigation and native day buttons in a Monday-first grid. Each day shows file count and recorded logical size; a small accent bar compares sizes within that month. Selected days use a border and pressed semantics, with the existing visible focus style. Unknown dates and outside-month totals are distinct. A bounded, keyboard-scrollable day table shows recorded paths, sizes, UTC modification times and the existing cloud labels. It offers no current-file action. Root size, save time, measurement time, coverage and accounting remain separate, and the copy distinguishes cohorts from bytes written or evidence of use.

Diary uses divided observation rows with explicit baseline, incompatible, partial and unknown-accounting states. Ranked paths describe recorded changes without attributing a cause or actor, and disclose overlapping folder totals. Inspect context opens the existing coordinated comparison at the opaque historical scope. The comparison owns its pair independently of the loaded selectors, so older diary pairs remain valid; Back to diary restores the anchor and page. Returning to captures preserves the user's capture selection. Opening a full snapshot is an explicit operation. Async replies and displaced-focus restoration require a current owner; loading, cancellation, failure, empty and operation-paused states stay distinct. Native rendered composition, pointer/keyboard/focus, scaling and device performance remain manual acceptance.

## Live observation strip

People monitoring an analyzed root can enable Live in a compact strip above the existing workspace. The root path and written state lead; Start Live changes to Resynchronize and Stop Live. The strip remains visible across navigation while active. It reuses Segoe UI, 12 px status text, theme surfaces, thin borders and existing button/focus styles, with wrapping controls and measurement rows. No animation, new palette or decorative panel is added. A selected folder follows its unique native directory identity when possible; an unavailable context returns to root. File selections and old indexed replies are invalidated on publication.

The secondary row shows the last root logical total, observed net change and both UTC measurement dates. Exact byte strings remain in titles; missing compatible evidence says Unavailable and unverified coverage is explicit. Lost events and requested resynchronization have written feedback. Starting, watching, stopping, hidden-stop and error states are distinct. Stop retains the last measurement and requires explicit restart. Control focus is restored only while its owner is current and focus was displaced. Native hiding/minimizing, page reload and component disposal request shutdown. Rendering, native keyboard/focus and scaling remain manual acceptance.

## Volume measurements

Welcome, Overview and the compact drive summary use measured whole-volume total, used and free counters. They never derive used space by mixing a caller quota with whole-volume free space. Available to you is a separate row. Existing rings render only when a positive measured total exists; unavailable counters remain explicit text. Overview and Welcome show the observation date, distinguishing a volume measurement from caller-only availability. Exact byte titles complement rounded labels. Existing surfaces, typography and spacing are retained; no new dashboard or palette is introduced. Native rendering, quota/device cases and scaling remain separate acceptance.

The populated Overview adds an initially closed Space account section below the map. Explain space requests the analyzed root's known file allocation, fresh volume used bytes and signed remaining difference. Selected drive remains a separate heading because its picker need not match the analyzed root. Three aligned values stack in narrow windows; exact-byte titles accompany rounded labels. Coverage, accounting revision support, exclusions, access issues and both measurement dates qualify the result. The difference is neither clamped nor described as reclaimable space.

Context and possible contributors is a secondary disclosure for hardlink paths, cloud metadata, unmeasured records, caller availability and independent reserve counters. Unmeasured explanations are labelled as possibilities. Unknown allocation and unavailable native reads remain explicit. Opening, reload, cancellation, pause and failure have distinct states; no idle query or file action is added. Controls reuse existing tokens and restore displaced focus only for the owning request. Rendered composition, keyboard/focus, assistive technology and Windows scaling remain manual acceptance under the user's no-computer-use instruction.

## Agent export permission

Agent connection creation exposes an independent, initially unchecked Export indexed metadata as CSV permission beside the existing metadata/scan controls. The native confirmation includes the resulting export capability. Existing connections are not upgraded. The backend enforces this permission independently of the form; rendering and native consent interaction remain manual acceptance.

## Recovery copy discovery

Overview places a closed Recovery copies section beside the existing space explanation. Query copies explicitly requests Windows metadata for the analyzed root's containing volume. The root and check date remain visible, with distinct loading, cancellation, pause, transport failure and native-unavailable states. The section reuses existing typography, panels, borders, buttons and focus styles. No copy query runs while closed or on an idle timer.

A compact table presents reported copy IDs, optional provider IDs, recorded UTC dates and written Yes/No/Unknown flags for Previous Versions and persistence. Long identifiers wrap within fixed table columns. An empty completed query says no records were returned; limited or timed-out enumeration explicitly remains incomplete. The first bounded provider prefix is sorted by known recorded date, without claiming it contains the newest copies. Advice directs people to Windows Previous Versions or their backup application's recovery controls and a separate destination. No restore, overwrite, purge or elevation action is introduced by discovery. Native rendering, populated providers, keyboard/focus, assistive technology and Windows scaling remain manual acceptance.

## Chart color and operation feedback refinement

The user explicitly keeps the stable sidebar behavior. The graph refinement changes category/ramp colors, not map geometry, filters, measurements, selection or saved color modes. The file-type donut, ranked rows, five shared map painters and graphic/history exports reuse the palette. Measured volume rings blend the existing blue role into violet within the actual used arc; the rest stays neutral. History adds a light area under the recorded line, separate latest-capture markers and a text legend. Growth/reduction badges pair color with Grew/Shrank text; unknown coverage retains explicit wording. Theme-aware chart line/badge roles supplement the existing accent, which remains reserved for interactions.

Operation progress is scoped to the current native operation ID. The core sends actual copy, destination-readback and source-recheck byte counters as decimal strings; the desktop coalesces updates while retaining phase boundaries. The UI rejects malformed or out-of-range counters rather than rounding. Recorded results separate confirmed copy counts, verified bytes, original outcomes, publication uncertainty and cleanup problems, with no promise of full metadata restoration or immediate freed space. Cleanup and the active-operation notice link directly to the existing human review section, with owned focus/scroll after navigation. The approval boundary and sidebar behavior are unchanged.

## Application location breakdown

Application evidence carries Current or Captured provenance. Live analysis reads current registration claims. A saved snapshot uses only the evidence recorded with it, keeping its registration-read window separate from the indexed measurement date. Older snapshots or failed evidence capture show an explicit unavailable state; they never fall back to the current Windows registry or display a measured zero. Existing app categories, colors, list, chart and scope controls are reused. Historical views keep cleanup disabled. Native visual, keyboard and scaling acceptance remain manual.

The supported HTTP cache category offers Review supported cache files for live analyses with a unique profile location and no overlapping application claims. It opens the existing Cleanup workspace with that folder scope and no automatic selection. The review states that Explorer file filters are not carried across and that only the existing seven-day external Blockfile recipe applies. Application name and the recipe lead the contextual heading; All cleanup rules restores the generic rule chooser. Category counts are storage measurements, not a promise that the recipe can clean that amount. Preview recomputes current application context, then uses the shared exact-file plan and human confirmation. Main sidebar behavior is unchanged. Native interaction and scaling acceptance remain manual.

Applications keeps its existing list, scoped chart, measured totals and navigation. Each row adds a closed native Location breakdown disclosure with installation, remaining profile data, supported HTTP cache, logs and unknown/ambiguous claims. Existing blue, gold, teal, pink and slate paints distinguish these categories. The thin row bar retains its global exclusive-byte scale and divides that measured share by category; shared claims remain separate, non-additive text. Missing categories say Not classified in this scope instead of displaying zero. Contextual, supplied and conflicting evidence are named, without invented percentages or cleanup permission.

The expanded definition list uses wrapping columns with tabular byte/count labels. Its summary has a visible accent focus ring; the native disclosure retains keyboard behavior. The current 1000px row breakpoint and stable sidebar are preserved. Profile data explicitly includes unclassified content; only supported current-user Windows default layouts are classified. There is no new cleaning action, palette, animation or dependency. Rendering, populated interaction, focus, assistive technology and Windows scaling remain manual under the no-Computer-Use instruction.

## Cleanup and recorded operations

Cleanup is available without a loaded scan. A compact button group selects Candidates, Activity, Agent plans or Transfers; only the active section mounts, so hidden sections perform no queries or polling. Candidates retain the existing rule/bin controls and still require a current scan before index-based actions. Activity retains historical journal restoration. Human agent review moves out of Settings into this workspace; Settings links to it beside connection permissions. Global progress links retain focus ownership when opening the review heading, and a recorded operation can request its exact plan by ID without granting authority.

Transfers uses a bounded list/detail composition with existing 20px spacing, 8px panel corners, blue/violet/teal status roles and a stacked layout below 1180px. The native list returns 20 rows; each file page returns at most 50. Source/destination previews are labelled as recorded context, while full paths, volume identity, dated caller capacity and metadata profile remain in the selected detail. Destination groups mount only when their disclosure opens. The file table scrolls within a keyboard-focusable region rather than widening the window. Loading, retry, empty, invalid-evidence and interrupted states have explicit text. Unknown connection/content status never becomes connected, zero or restorable. Recorded publication counts do not claim current file availability or freed space. The sidebar and chart palette remain unchanged.

## Reading and control hierarchy

The storage workbench uses open sections, aligned rows and separators instead of a framed card for every information group. Overview and application sections use plain secondary navigation with an active underline. Summary headings use sentence case and existing title sizes; list text uses 13px and supporting metadata generally 12px. The black theme, chart palette, original artwork and main sidebar behavior remain in place.

The header groups location controls and scoped search. Search and its advanced editor appear in indexed-file views, where they apply. Saved views stay globally accessible. The first scan remains a primary action; Rescan is secondary after an index is available. Secondary header actions are quiet, with visible hover and keyboard focus.

The map keeps a native representation selector, List/Grid, current selection tools, export and inspection directly available. Area, depth, cloud filtering, height and color live in a native View options disclosure. Its closed summary still names the active encodings and indicates a cloud filter. No filter, selection or scan behavior changes when opening this disclosure. Map navigation controls and operations sections use underlines instead of nested outlined button groups.

The application scatter keeps a readable vector width and larger axis text, using a horizontal scroll region on narrower windows. Point and axis geometry share one definition. Keyboard point focus scrolls the relevant point into view, and the measured list remains the alternative. Responsive summary separators follow the one- or two-column layout. Rendering, keyboard interaction and Windows scaling remain manual under the user's no-Computer-Use instruction; source checks and a successful build do not establish visual acceptance.

View files in Applications opens the existing Explorer table within the current folder, adding the application ID to the applied expression. It keeps current filters and includes all matching installation/profile locations in that folder. The chart and list use the same action. No extra action row or nested button group is introduced. Advanced search offers one concise application example; evidence dates distinguish current registration reads, frozen analysis observations and saved snapshot evidence.

## Cleanup review tray

Review tray sits beside Saved in the workspace toolbar. Its count stays visible across views. The tray opens below the active workspace as a compact section with source disclosures, folder headings and aligned file rows. Candidate pages use one Add to review tray action. Explorer checkboxes update the tray directly. Preparation opens the existing native confirmation dialog and lists the sources and retained copies.

The simulation shows indexed logical size and the remaining logical size on the same scale. A separate figure shows known indexed allocation. Review target is an optional disclosure for logical size or indexed allocation. The tray uses the existing light and black theme tokens, visible focus and compact controls. Columns stack at narrower widths. Populated rendering, keyboard interaction and Windows scaling require manual review.
