"""Developer-only independent inspection of the retained owned WebM publication fixture."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
from datetime import datetime, timezone


def elements(data):
    position = 0
    while position < len(data):
        def vint(identifier=False):
            nonlocal position
            width = 9 - data[position].bit_length()
            assert 1 <= width <= (4 if identifier else 8)
            raw = data[position:position + width]
            assert len(raw) == width
            position += width
            value = int.from_bytes(raw, 'big')
            return value if identifier else value & ((1 << (7 * width)) - 1)

        identifier, size = vint(True), vint()
        if identifier == 0x18538067:
            size = min(size, len(data) - position)
        assert position + size <= len(data)
        yield identifier, data[position:position + size]
        position += size


def metadata(data):
    segment = dict(elements(data))[0x18538067]
    tags = [value for key, value in elements(segment) if key == 0x1254C367]
    assert len(tags) == 1
    tag = dict(elements(tags[0]))[0x7373]
    parts = dict(elements(tag))
    assert parts[0x63C0] == b''
    simple = dict(elements(parts[0x67C8]))
    assert simple[0x45A3] == b'VOLDIQ_CONTEXT'
    return json.loads(simple[0x4487])


def decode(encoder, source):
    result = subprocess.run([
        str(encoder), '-hide_banner', '-nostdin', '-loglevel', 'info',
        '-protocol_whitelist', 'file,pipe', '-i', str(source), '-map', '0:v:0',
        '-vf', 'showinfo', '-fps_mode', 'passthrough', '-pix_fmt', 'rgb24',
        '-f', 'rawvideo', 'pipe:1',
    ], capture_output=True, check=True, timeout=30)
    times = re.findall(r'\bpts_time:([0-9.]+)', result.stderr.decode('utf-8', errors='replace'))
    assert times == ['0', '0.5'], times
    assert len(result.stdout) == 2 * 16 * 16 * 3
    return result.stdout, times


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ffmpeg', type=Path, required=True)
    parser.add_argument('--published', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    assert args.ffmpeg.is_absolute() and args.published.is_absolute()
    assert not args.ffmpeg.is_symlink() and not args.published.is_symlink()
    assert hashlib.sha256(args.ffmpeg.read_bytes()).hexdigest() == (
        '3256173f3f8bffd7df12227c68adf68025edb1832273a9530688a7bb1ed8edec'
    )
    repository = Path(__file__).resolve().parent.parent
    sources = repository / 'crates/voldiq-core/tests/fixtures'
    frames = []
    for name, original in [
        ('vp8.webm', 'history-vp8.webm'),
        ('vp8-live.webm', 'history-vp8-live.webm'),
        ('vp9-live.webm', 'history-vp9-live.webm'),
        ('unknown-cluster.webm', 'history-vp8.webm'),
    ]:
        path = args.published / name
        assert path.is_file() and not path.is_symlink()
        data = path.read_bytes()
        context = metadata(data)
        assert context['format'] == 'webm'
        assert context['width'] == context['height'] == 16
        assert context['video_blocks'] == 2
        assert context['first_timestamp_ns'] == '0'
        assert context['last_timestamp_ns'] == '500000000'
        assert context['clip']['paths_included'] is False
        assert context['clip']['root_path'] is None
        assert [frame['logical'] for frame in context['clip']['frames']] == ['33', '61']
        assert b'input.invalid' not in data and b'source-client-metadata-marker' not in data
        pixels, times = decode(args.ffmpeg, path)
        original_pixels, _ = decode(args.ffmpeg, sources / original)
        assert pixels == original_pixels
        frames.append({
            'file': name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(),
            'codec': context['codec'], 'frames': 2, 'width': 16, 'height': 16,
            'timestampsSeconds': times, 'decodedPixelsExactlyPreserved': True,
            'nativeLogicalBytes': ['33', '61'], 'pathsWithheld': True,
            'inputTextMetadataDiscarded': True,
        })
    receipt = {
        'checkedUtc': datetime.now(timezone.utc).isoformat(),
        'scope': 'Owned core publisher outputs decoded independently; no WebView recorder, IPC, dialog, UI, seeking or performance acceptance.',
        'developmentEncoder': {
            'package': 'imageio-ffmpeg 0.8.1 official PyPI wheel',
            'wheelSha256': 'f27ca4cb85e2d4946deedbce1a81245dfd942a2bcf94b53676fb3956aa95bf1b',
            'executableSha256': hashlib.sha256(args.ffmpeg.read_bytes()).hexdigest(),
            'productionDependency': False,
        },
        'fixtureDirectory': str(args.published), 'files': frames,
    }
    args.receipt.write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'files': len(frames), 'receipt': str(args.receipt)}))


if __name__ == '__main__':
    main()
