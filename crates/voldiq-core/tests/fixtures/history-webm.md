# Owned WebM fixtures

The three `history-*.webm` files contain two synthetic 16 by 16 RGB frames:
`(33, 40, 50)` and `(61, 40, 50)`, each held for 500 milliseconds. They contain
no recorded user content. The input title and track name deliberately contain
`https://input.invalid/private?secret=marker` and
`source-client-metadata-marker` so publication can prove that client text is
discarded. These markers are fictional.

They were generated with FFmpeg 9.0.2, from the verified Windows executable in
the official `imageio-ffmpeg` 0.8.1 PyPI wheel. The encoder is a developer-only
probe in ignored `target/probes`, never an application or routine test dependency.
The wheel SHA-256 is
`f27ca4cb85e2d4946deedbce1a81245dfd942a2bcf94b53676fb3956aa95bf1b`.

Write `bytes([33,40,50])*256 + bytes([61,40,50])*256` to `frames.rgb`, then run:

```text
ffmpeg -f rawvideo -pixel_format rgb24 -video_size 16x16 -framerate 2 -i frames.rgb -an -c:v libvpx -threads 1 -b:v 100k -metadata title=https://input.invalid/private?secret=marker -metadata:s:v:0 title=source-client-metadata-marker history-vp8.webm
```

For `history-vp8-live.webm`, add `-live 1` before the destination. For
`history-vp9-live.webm`, also replace `libvpx` with `libvpx-vp9`. Muxer-generated
identifiers can differ on regeneration. All three files are decoded independently
after the native publisher remuxes them; fixtures do not stand in for acceptance
of a particular WebView's recorder.

References: [package](https://pypi.org/project/imageio-ffmpeg/0.8.1/),
[WebM container](https://www.webmproject.org/docs/container/),
[Matroska elements](https://www.matroska.org/technical/elements.html).
