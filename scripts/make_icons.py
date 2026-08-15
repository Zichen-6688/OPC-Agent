#!/usr/bin/env python3
"""生成 OPC Agent 应用图标:蓝色渐变圆角方块 + 白色调度网络图(中心=调度中枢,四周=AI员工)。
输出 1024px 主图,再转出 Tauri 所需全部尺寸。
"""
import math
import os
import subprocess
import sys
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
ICONS = os.path.join(HERE, "..", "src-tauri", "icons")
TMP = os.path.join(HERE, ".icon_tmp")

SIZE = 1024
RADIUS = 220


def lerp(a, b, t):
    return a + (b - a) * t


def gradient(w, h, top, bottom):
    img = Image.new("RGB", (w, h))
    d = ImageDraw.Draw(img)
    for y in range(h):
        t = y / h
        color = tuple(int(lerp(top[i], bottom[i], t)) for i in range(3))
        d.line([(0, y), (w, y)], fill=color)
    return img


def rounded_mask(w, h, r):
    mask = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(mask)
    d.rounded_rectangle([0, 0, w - 1, h - 1], radius=r, fill=255)
    return mask


def main():
    os.makedirs(TMP, exist_ok=True)

    # 背景渐变
    bg = gradient(SIZE, SIZE, (59, 130, 246), (29, 78, 216))
    bg.putalpha(255)
    mask = rounded_mask(SIZE, SIZE, RADIUS)
    icon = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    icon.paste(bg, (0, 0), mask)

    d = ImageDraw.Draw(icon)

    # 内发光圆环(比背景略亮的描边)
    d.rounded_rectangle([26, 26, SIZE - 27, SIZE - 27], radius=RADIUS - 26,
                        outline=(147, 197, 253, 120), width=6)

    # ---- 调度网络图 ----
    cx, cy = SIZE / 2, SIZE / 2
    center_r = 118
    sat_r = 88
    sat_d = 300          # 卫星节点到中心距离
    angles = [-90, 30, 150]  # 上、右下、左下

    nodes = [(cx, cy)] + [
        (cx + sat_d * math.cos(math.radians(a)),
         cy + sat_d * math.sin(math.radians(a)))
        for a in angles
    ]

    # 连线(半透明白)
    for x, y in nodes[1:]:
        d.line([(cx, cy), (x, y)], fill=(255, 255, 255, 110), width=16)

    # 中心节点:白色圆 + 蓝色内圆
    d.ellipse([cx - center_r, cy - center_r, cx + center_r, cy + center_r], fill=(255, 255, 255, 255))
    d.ellipse([cx - center_r + 26, cy - center_r + 26, cx + center_r - 26, cy + center_r - 26],
              fill=(37, 99, 235, 255))

    # 中心大脑符号:三道弧形(简化脑波)
    for i, amp in enumerate([46, 66, 46]):
        base_y = cy - 20 + i * 20
        pts = []
        for t in range(0, 25):
            x = cx - 60 + t * 5
            y = base_y + amp * math.sin(t / 24 * math.pi)
            pts.append((x, y))
        d.line(pts, fill=(255, 255, 255, 255), width=13)

    # 卫星节点:白圆 + 员工色点
    for i, (x, y) in enumerate(nodes[1:]):
        d.ellipse([x - sat_r, y - sat_r, x + sat_r, y + sat_r], fill=(255, 255, 255, 255))
        colors = [(37, 99, 235), (124, 58, 237), (13, 148, 136)]
        c = colors[i % 3]
        d.ellipse([x - sat_r + 22, y - sat_r + 22, x + sat_r - 22, y + sat_r - 22], fill=c)
        # 每个卫星里的"人"简化:圆头 + 半圆身
        hr = 22
        hx, hy = x, y - 6
        d.ellipse([hx - hr, hy - hr, hx + hr, hy + hr], fill=(255, 255, 255, 235))
        d.pieslice([x - 34, y + 2, x + 34, y + 58], 180, 360, fill=(255, 255, 255, 235))

    png_1024 = os.path.join(TMP, "icon-1024.png")
    icon.save(png_1024, "PNG")
    print("master icon:", png_1024)

    # ---- 输出各尺寸 PNG ----
    os.makedirs(ICONS, exist_ok=True)
    sizes = {
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon.png": 512,
        "Square30x30Logo.png": 30,
        "Square44x44Logo.png": 44,
        "Square71x71Logo.png": 71,
        "Square89x89Logo.png": 89,
        "Square107x107Logo.png": 107,
        "Square142x142Logo.png": 142,
        "Square150x150Logo.png": 150,
        "Square284x284Logo.png": 284,
        "Square310x310Logo.png": 310,
        "StoreLogo.png": 50,
    }
    for name, sz in sizes.items():
        icon.resize((sz, sz), Image.LANCZOS).save(os.path.join(ICONS, name), "PNG")

    # ---- .ico(Windows,含多尺寸) ----
    ico_sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    icon.resize((256, 256), Image.LANCZOS).save(
        os.path.join(ICONS, "icon.ico"),
        format="ICO",
        sizes=ico_sizes,
    )

    # ---- .icns(macOS,经 iconset + iconutil) ----
    iconset = os.path.join(TMP, "icon.iconset")
    os.makedirs(iconset, exist_ok=True)
    icns_sizes = [
        ("icon_16x16.png", 16), ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32), ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128), ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256), ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512), ("icon_512x512@2x.png", 1024),
    ]
    for name, sz in icns_sizes:
        icon.resize((sz, sz), Image.LANCZOS).save(os.path.join(iconset, name), "PNG")
    subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(ICONS, "icon.icns")], check=True)

    print("icons written to", ICONS)
    for f in sorted(os.listdir(ICONS)):
        print(" ", f)


if __name__ == "__main__":
    sys.exit(main())
