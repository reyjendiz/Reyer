"""Flat file-type icons: a coloured rounded square with one white glyph. Video = play, audio = note."""
import os, sys
from PIL import Image, ImageDraw, ImageFilter, ImageChops

out = sys.argv[1]
W, S = 1024, 2  # drawn at 2x, scaled down
SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]


def tile(top, bottom):
    """Frosted glass tile: tinted gradient, light pooling top-left, soft sheen, bright rim, drop shadow."""
    n = W * S
    box = [48 * S, 40 * S, (W - 48) * S, (W - 56) * S]
    rad = 230 * S
    shape = Image.new('L', (n, n), 0)
    ImageDraw.Draw(shape).rounded_rectangle(box, radius=rad, fill=255)
    out = Image.new('RGBA', (n, n), (0, 0, 0, 0))
    # soft drop shadow
    sh = Image.new('RGBA', (n, n), (0, 0, 0, 0))
    sm = Image.new('L', (n, n), 0)
    ImageDraw.Draw(sm).rounded_rectangle([box[0] + 10 * S, box[1] + 34 * S, box[2] - 10 * S, box[3] + 34 * S], radius=rad, fill=110)
    sh.putalpha(sm.filter(ImageFilter.GaussianBlur(28 * S)))
    out = Image.alpha_composite(out, sh)
    # tinted body, slightly translucent
    body = Image.new('RGBA', (n, n))
    bd = ImageDraw.Draw(body)
    for y in range(n):
        t = y / n
        bd.line([(0, y), (n, y)], fill=tuple(int(top[i] * (1 - t) + bottom[i] * t) for i in range(3)) + (232,))
    body.putalpha(ImageChops.multiply(body.getchannel('A'), shape))
    out = Image.alpha_composite(out, body)
    # light pooling in the top-left corner
    glow = Image.radial_gradient('L').resize((n, n)).point(lambda v: 255 - v)
    glow = ImageChops.offset(glow, -n // 5, -n // 5)
    gl = Image.new('RGBA', (n, n), (255, 255, 255, 0))
    gl.putalpha(ImageChops.multiply(glow.point(lambda v: int(v * 0.42)), shape))
    out = Image.alpha_composite(out, gl)
    # top sheen: a pale band over the upper third that fades out
    sheen = Image.new('L', (n, n), 0)
    ImageDraw.Draw(sheen).rounded_rectangle([box[0] + 26 * S, box[1] + 22 * S, box[2] - 26 * S, box[1] + 330 * S], radius=rad - 30 * S, fill=70)
    sheen = sheen.filter(ImageFilter.GaussianBlur(26 * S))
    sg = Image.new('RGBA', (n, n), (255, 255, 255, 0))
    sg.putalpha(ImageChops.multiply(sheen, shape))
    out = Image.alpha_composite(out, sg)
    # glass rim: bright on the top-left edge, faint on the bottom-right
    rim = Image.new('L', (n, n), 0)
    ImageDraw.Draw(rim).rounded_rectangle(box, radius=rad, outline=255, width=5 * S)
    ramp = Image.linear_gradient('L').resize((n, n)).rotate(45, resample=Image.BICUBIC).point(lambda v: 255 - v)
    rim_a = ImageChops.multiply(rim, ramp.point(lambda v: 25 + int(v * 0.6)))
    rg = Image.new('RGBA', (n, n), (255, 255, 255, 0))
    rg.putalpha(rim_a)
    out = Image.alpha_composite(out, rg)
    return out


def glyph_layer(draw_fn):
    """White glyph with a soft shadow, translucent like etched glass."""
    n = W * S
    m = Image.new('L', (n, n), 0)
    draw_fn(ImageDraw.Draw(m))
    shadow = Image.new('RGBA', (n, n), (40, 20, 60, 0))
    shadow.putalpha(ImageChops.offset(m, 0, 14 * S).filter(ImageFilter.GaussianBlur(16 * S)).point(lambda v: int(v * 0.35)))
    g = Image.new('RGBA', (n, n), (255, 255, 255, 0))
    g.putalpha(m.point(lambda v: int(v * 0.95)))
    return shadow, g


def finish(img, name):
    img = img.resize((W, W), Image.LANCZOS)
    img.save(os.path.join(out, name + '.png'))
    img.save(os.path.join(out, name + '.ico'), sizes=SIZES)


P = lambda x, y: (x * S, y * S)
WHITE = (255, 255, 255, 255)

# video: coral glass tile, white rounded play triangle
c = (517, 512)
pts = [(c[0] + (x - c[0]) * 0.74, c[1] + (y - c[1]) * 0.74) for x, y in [(395, 300), (395, 724), (760, 512)]]
sp = [P(*p) for p in pts]


def play(d):
    d.polygon(sp, fill=255)
    d.line(sp + [sp[0], sp[1]], fill=255, width=96 * S, joint='curve')
    for p in sp:
        d.ellipse([p[0] - 48 * S, p[1] - 48 * S, p[0] + 48 * S, p[1] + 48 * S], fill=255)


def note(d):
    for cx, cy in [(398, 678), (648, 628)]:
        d.ellipse([P(cx - 84, cy - 66), P(cx + 84, cy + 66)], fill=255)
    d.rectangle([P(438, 318), P(484, 670)], fill=255)
    d.rectangle([P(688, 300), P(734, 620)], fill=255)
    d.polygon([P(438, 318), P(734, 268), P(734, 372), P(438, 422)], fill=255)


for name, top, bottom, fn in [('reyer_video', (255, 150, 105), (238, 72, 58), play), ('reyer_audio', (150, 125, 255), (80, 58, 220), note)]:
    img = tile(top, bottom)
    shadow, g = glyph_layer(fn)
    img = Image.alpha_composite(Image.alpha_composite(img, shadow), g)
    finish(img, name)
print('done')
