"""Produce Windows icon sizes from the supplied artwork without redrawing it."""
from pathlib import Path
from PIL import Image, PngImagePlugin

root = Path(__file__).resolve().parents[1]
metadata = PngImagePlugin.PngInfo()
metadata.add_text('Source', 'User-provided Voldiq artwork, attachment image-1.png and image-2.png, 2026-10-11. Only crop and resize for technical packaging.')
metadata.add_text('impeccable:prompt', 'Origin: user-provided Voldiq artwork (image-1.png and image-2.png), 2026-10-11. Technical crop, Lanczos resizing, Windows ICO packaging and removal of the white matte from the derived sidebar symbol. The original colored artwork and source files are preserved.')
icon = Image.open(root / 'assets/icon-original.png').convert('RGBA').crop((195, 195, 1055, 1055))
for size in (32, 128, 256):
    icon.resize((size, size), Image.Resampling.LANCZOS).save(root / f'src-tauri/icons/{size}x{size}.png', pnginfo=metadata)
icon.save(root / 'src-tauri/icons/icon.ico', sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
original = Image.open(root / 'assets/wordmark-original.png').convert('RGBA')
original.crop((195,285,1485,670)).save(root / 'frontend/public/wordmark.png', pnginfo=metadata)
symbol = original.crop((195,285,575,665))
pixels = symbol.load()
for y in range(symbol.height):
    for x in range(symbol.width):
        red, green, blue, alpha = pixels[x, y]
        if min(red, green, blue) >= 235:
            alpha = round(alpha * max(0, (245 - min(red, green, blue)) / 10))
        pixels[x, y] = red, green, blue, alpha
symbol.resize((128,128), Image.Resampling.LANCZOS).save(root / 'frontend/public/symbol.png', pnginfo=metadata)
