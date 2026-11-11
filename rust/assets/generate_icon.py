"""Recreate the Python settings icon as a Windows multiresolution icon."""
from pathlib import Path
from PIL import Image, ImageDraw

size = 256
scale = size / 64
image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
draw = ImageDraw.Draw(image)

def rect(bounds, radius, fill):
    draw.rounded_rectangle(tuple(round(v * scale) for v in bounds), round(radius * scale), fill)

rect((4, 4, 60, 60), 14, (30, 34, 44, 255))
rect((14, 18, 38, 34), 4, (255, 135, 103, 255))
rect((28, 32, 52, 48), 4, (150, 160, 180, 255))
image.save(Path(__file__).with_name("appswitcher.ico"), sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
