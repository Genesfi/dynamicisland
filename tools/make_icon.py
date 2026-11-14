from PIL import Image, ImageDraw

def create_icon():
    size = 1024
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Palette 1: Deep obsidian dark slate
    c_dark = (15, 19, 28, 255)
    # Palette 2: Pure crisp white
    c_white = (255, 255, 255, 255)

    # 1. Outer rounded squircle
    margin = 56
    r_corner = 224
    draw.rounded_rectangle([margin, margin, size - margin, size - margin], radius=r_corner, fill=c_dark)

    # Subtle inner rim highlight
    c_rim = (38, 46, 62, 255)
    draw.rounded_rectangle([margin, margin, size - margin, size - margin], radius=r_corner, outline=c_rim, width=14)

    # 2. Dynamic Island White Capsule in Center
    cx, cy = size // 2, size // 2
    pw, ph = 620, 260
    pill_left = cx - pw // 2
    pill_top = cy - ph // 2
    pill_right = cx + pw // 2
    pill_bottom = cy + ph // 2
    draw.rounded_rectangle([pill_left, pill_top, pill_right, pill_bottom], radius=ph // 2, fill=c_white)

    # 3. Cutouts inside the white pill (Camera sensor on left, soundwave bar on right)
    cam_cx = cx - pw // 4 + 10
    cam_r = 46
    draw.ellipse([cam_cx - cam_r, cy - cam_r, cam_cx + cam_r, cy + cam_r], fill=c_dark)

    # Small sensor dot next to camera
    dot_cx = cam_cx + 80
    dot_r = 18
    draw.ellipse([dot_cx - dot_r, cy - dot_r, dot_cx + dot_r, cy + dot_r], fill=c_dark)

    # Soundwave bars cutout on right
    bar_cx = cx + pw // 4 - 30
    bw, bh = 26, 96
    draw.rounded_rectangle([bar_cx - bw//2, cy - bh//2, bar_cx + bw//2, cy + bh//2], radius=13, fill=c_dark)

    bar2_cx = bar_cx + 44
    b2w, b2h = 24, 62
    draw.rounded_rectangle([bar2_cx - b2w//2, cy - b2h//2, bar2_cx + b2w//2, cy + b2h//2], radius=12, fill=c_dark)

    # Save master PNG and ICO with multiple resolutions
    img.save("assets/app_icon.png", format="PNG")

    sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    img.save("assets/app.ico", format="ICO", sizes=sizes)
    print("Icons successfully created in assets/app.ico and assets/app_icon.png")

if __name__ == "__main__":
    create_icon()
