#!/usr/bin/env python3
"""Generate all icon files from logo.png — single source of truth.

Source: /home/juan/RojoClaude/logo.png (1024x1024 RGBA, rounded corners)
This script resizes it to all sizes needed by Tauri + frontend.
"""
import os
import shutil
from PIL import Image

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
SOURCE = os.path.join(SCRIPT_DIR, "..", "..", "..", "logo.png")

# All PNG sizes needed by Tauri
PNG_SIZES = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
}

# Also update the frontend asset
FRONTEND_ICON = os.path.join(SCRIPT_DIR, "..", "..", "src", "assets", "icon.png")


def resize(source_img: Image.Image, size: int) -> Image.Image:
    """Resize with high-quality LANCZOS resampling."""
    return source_img.resize((size, size), Image.LANCZOS)


def main():
    if not os.path.exists(SOURCE):
        print(f"ERROR: Source not found: {SOURCE}")
        return

    source = Image.open(SOURCE).convert("RGBA")
    print(f"Source: {SOURCE} ({source.width}x{source.height})")
    print()

    # Generate PNGs
    for filename, size in PNG_SIZES.items():
        output_path = os.path.join(SCRIPT_DIR, filename)
        img = resize(source, size)
        img.save(output_path, format="PNG")
        print(f"  {filename} ({size}x{size})")

    # Generate frontend asset
    img = resize(source, 256)
    os.makedirs(os.path.dirname(FRONTEND_ICON), exist_ok=True)
    img.save(FRONTEND_ICON, format="PNG")
    print(f"  src/assets/icon.png (256x256)")

    # Generate ICO (multiple sizes)
    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    ico_images = [resize(source, s) for s in ico_sizes]
    ico_path = os.path.join(SCRIPT_DIR, "icon.ico")
    ico_images[0].save(
        ico_path,
        format="ICO",
        sizes=[(s, s) for s in ico_sizes],
        append_images=ico_images[1:],
    )
    print(f"  icon.ico ({', '.join(str(s) for s in ico_sizes)})")

    # Generate ICNS
    icns_path = os.path.join(SCRIPT_DIR, "icon.icns")
    try:
        source.save(icns_path, format="ICNS")
        print(f"  icon.icns")
    except Exception:
        print(f"  Warning: Cannot generate .icns (Pillow ICNS support missing)")

    print()
    print("Done. All icons generated from logo.png")


if __name__ == "__main__":
    main()
