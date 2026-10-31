# Treemap navigation motion

- Purpose: explain which measured rectangle becomes the next folder, and where that folder sits when returning to its parent.
- Direction: preserve the existing artwork, palette, typography, proportional layout and controls. Animate only pointer-initiated hierarchy changes with a visible source/destination rectangle. Ordinary scan refreshes, metric changes, keyboard navigation, first render and unrepresented destinations stay immediate.
- Implementation: a 160 ms finite animation crossfades the bounded old/new layouts while expanding from or contracting into the corresponding rectangle. Repaint existing vector geometry rather than capturing canvas bitmaps. No dependency, extra copy or idle frame loop.
- Controls: new input interrupts the animation and commits the latest map before hit testing. Resize, replacement, unmount, hidden document, application Reduce motion and system reduced-motion preference cancel outstanding frames. Accessible list navigation stays immediate.
- Verification: extend the retained frontend case with literal zoom geometry and a deterministic frame clock that proves termination, cancellation and stale-callback rejection. Keep eight declared tests. Source gate and native preview/package refresh are allowed; visual acceptance remains user-owned, with no Computer Use or subagents.

Result: implemented and included in the corrected production image. Literal geometry, finite termination, idle stopping and stale-callback rejection pass in the retained frontend case. The stale-frame mutation fails as expected and is restored. All six production-mode source checks pass with eight declared tests. Visual acceptance remains manual.
