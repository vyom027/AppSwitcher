"""
Picker layouts for AppSwitcher. Each layout is a class with:
    build()                  -> create QGraphicsItems in the scene
    update(focus, selected)  -> position/scale items (focus is a float)
    hit_test(px, py)         -> app index under the point, or None

The picker shell (switcher.show_picker) owns the backdrop, hint, finger/keyboard
polling, fade-in, and commit; it just instantiates the configured layout.
"""
import math
from PIL import Image
from PySide6 import QtCore, QtGui, QtWidgets

import switcher as S

NoBrush = QtCore.Qt.NoBrush
YAxis   = QtCore.Qt.YAxis


def _card_pix(im, w, h, r=14):
    w, h = max(1, int(w)), max(1, int(h))
    return S._pil2pix(S._rounded_rgba(im.resize((w, h), Image.BILINEAR), r))


class BaseLayout:
    def __init__(self, scene, imgs, windows, sw, sh, accent):
        self.scene, self.imgs, self.windows = scene, imgs, windows
        self.sw, self.sh, self.accent = sw, sh, accent
        self.n = len(imgs)
        self.items = []

    # helpers ----------------------------------------------------------------
    def _ring(self, z=5, width=3):
        r = self.scene.addPath(QtGui.QPainterPath(), QtGui.QPen(self.accent, width))
        r.setBrush(NoBrush); r.setZValue(z)
        return r

    def _ring_rect(self, ring, x, y, w, h, rad=14):
        p = QtGui.QPainterPath(); p.addRoundedRect(x, y, w, h, rad, rad)
        ring.setPath(p)

    def _title(self, z=6, size=15):
        f = QtGui.QFont("Segoe UI", size); f.setBold(True)
        t = self.scene.addText("", f)
        t.setDefaultTextColor(QtGui.QColor(245, 248, 255)); t.setZValue(z)
        return t

    def _set_title(self, item, sel, y):
        s = self.windows[sel][1]
        item.setPlainText(s[:54] + ("…" if len(s) > 54 else ""))
        item.setPos((self.sw - item.boundingRect().width()) / 2, y)

    def build(self): ...
    def update(self, focus, selected): ...
    def hit_test(self, px, py): return None

    # live thumbnails: swap one card's image in place (same size/position).
    # build() sets self._cardwh = (w, h, radius) for this to work.
    def refresh_image(self, i, im):
        if im is None or not (0 <= i < self.n) or i >= len(self.items):
            return
        self.imgs[i] = im
        cw = getattr(self, "_cardwh", None)
        if cw:
            self.pix[i] = _card_pix(im, *cw)
            self.items[i].setPixmap(self.pix[i])
        p = getattr(self, "preview", None)        # Dock/Hero big preview
        if p is not None:
            p.imgs[i] = im
            p.cache.pop(i, None)                  # force re-render


# ── shared big-preview widget (used by Dock + Hero) ──────────────────────────
class _Preview:
    def __init__(self, scene, imgs, windows, sw, top, maxh, maxw, accent, title_y):
        self.scene, self.imgs, self.windows = scene, imgs, windows
        self.sw, self.top, self.maxh, self.maxw = sw, top, maxh, maxw
        self.title_y = title_y
        self.cache = {}
        self.refl = scene.addPixmap(QtGui.QPixmap()); self.refl.setZValue(1)
        self.prev = scene.addPixmap(QtGui.QPixmap()); self.prev.setZValue(2)
        glow = QtWidgets.QGraphicsDropShadowEffect()
        glow.setColor(accent); glow.setBlurRadius(46); glow.setOffset(0, 0)
        self.prev.setGraphicsEffect(glow)
        self.ring = scene.addPath(QtGui.QPainterPath(), QtGui.QPen(accent, 3))
        self.ring.setBrush(NoBrush); self.ring.setZValue(3)
        f = QtGui.QFont("Segoe UI", 15); f.setBold(True)
        self.title = scene.addText("", f)
        self.title.setDefaultTextColor(QtGui.QColor(245, 248, 255)); self.title.setZValue(4)
        self.rect = (0, 0, 0, 0)

    def _pix(self, i):
        if i not in self.cache:
            im = self.imgs[i]; aspect = im.width / max(1, im.height)
            pw = min(self.maxw, int(self.maxh * aspect)); ph = int(pw / aspect)
            if ph > self.maxh:
                ph = self.maxh; pw = int(ph * aspect)
            c = S._rounded_rgba(im.resize((max(1, pw), max(1, ph)), Image.BILINEAR), 22)
            r = S._reflection(c, frac=0.28, top_alpha=95)
            self.cache[i] = (S._pil2pix(c), S._pil2pix(r), pw, ph)
        return self.cache[i]

    def show(self, sel):
        cpix, rpix, pw, ph = self._pix(sel)
        px = self.sw // 2 - pw // 2
        py = self.top + (self.maxh - ph) // 2
        self.prev.setPixmap(cpix); self.prev.setPos(px, py)
        self.refl.setPixmap(rpix); self.refl.setPos(px, py + ph + 4)
        p = QtGui.QPainterPath(); p.addRoundedRect(px, py, pw, ph, 22, 22)
        self.ring.setPath(p)
        s = self.windows[sel][1]
        self.title.setPlainText(s[:54] + ("…" if len(s) > 54 else ""))
        self.title.setPos((self.sw - self.title.boundingRect().width()) / 2, self.title_y)
        self.rect = (px, py, pw, ph)


# ── 1. Dock ──────────────────────────────────────────────────────────────────
class DockLayout(BaseLayout):
    DOCK_W, DOCK_H, GAP = 150, 94, 22

    def build(self):
        sw, sh = self.sw, self.sh
        self.mag = float(S.SETTINGS.get("dock_mag", 1.95))
        maxh = int(self.DOCK_H * self.mag)
        self.basey = sh - 40
        title_y = self.basey - maxh - 50
        self.preview = _Preview(self.scene, self.imgs, self.windows, sw, 52,
                                title_y - 22 - 52, int(sw * 0.88), self.accent, title_y)
        self._cardwh = (self.DOCK_W, self.DOCK_H, 12)
        self.pix = [_card_pix(im, self.DOCK_W, self.DOCK_H, 12) for im in self.imgs]
        for i in range(self.n):
            it = self.scene.addPixmap(self.pix[i]); it.setZValue(2)
            it.setTransformOriginPoint(0, 0)
            self.items.append(it)
        self.ring = self._ring(3)

    def _m(self, i, focus):
        d = abs(i - focus)
        return 1.0 + (self.mag - 1.0) * math.exp(-(d * d) / 1.6)

    def update(self, focus, selected):
        self.preview.show(selected)
        ws = [self.DOCK_W * self._m(i, focus) for i in range(self.n)]
        x = (self.sw - (sum(ws) + (self.n - 1) * self.GAP)) / 2
        for i in range(self.n):
            m = self._m(i, focus); it = self.items[i]
            it.setScale(m); it.setPos(x, self.basey - self.DOCK_H * m)
            it.setOpacity(1.0 if i == selected else 0.80)
            if i == selected:
                self._ring_rect(self.ring, x, self.basey - self.DOCK_H * m,
                                self.DOCK_W * m, self.DOCK_H * m, 12)
            x += ws[i] + self.GAP

    def hit_test(self, px, py):
        ws = [self.DOCK_W * self._m(i, i) for i in range(self.n)]   # rough
        x = (self.sw - (sum(ws) + (self.n - 1) * self.GAP)) / 2
        for i in range(self.n):
            w = ws[i]
            if x <= px <= x + w and self.basey - self.DOCK_H <= py <= self.basey:
                return i
            x += w + self.GAP
        return None


# ── 2. Row ───────────────────────────────────────────────────────────────────
class RowLayout(BaseLayout):
    CW, CH, GAP = 240, 150, 30

    def build(self):
        self.cy = self.sh / 2 - 20
        total = self.n * self.CW + (self.n - 1) * self.GAP
        fit = min(1.0, (self.sw - 200) / total) if total else 1.0
        self.cw, self.ch = int(self.CW * fit), int(self.CH * fit)
        self.gap = int(self.GAP * fit)
        self.x0 = (self.sw - (self.n * self.cw + (self.n - 1) * self.gap)) / 2
        self._cardwh = (self.cw, self.ch, 14)
        self.pix = [_card_pix(im, self.cw, self.ch, 14) for im in self.imgs]
        for i in range(self.n):
            it = self.scene.addPixmap(self.pix[i]); it.setZValue(2)
            it.setTransformOriginPoint(self.cw / 2, self.ch / 2)
            self.items.append(it)
        self.ring = self._ring(3)
        self.titem = self._title()

    def _cx(self, i):
        return self.x0 + i * (self.cw + self.gap) + self.cw / 2

    def update(self, focus, selected):
        for i in range(self.n):
            s = 1 + 0.24 * math.exp(-((i - focus) ** 2) / 0.5)
            it = self.items[i]
            it.setScale(s); it.setPos(self._cx(i) - self.cw / 2, self.cy - self.ch / 2)
            it.setOpacity(1.0 if i == selected else 0.85)
        ms = 1 + 0.24
        cx = self._cx(selected)
        self._ring_rect(self.ring, cx - self.cw * ms / 2, self.cy - self.ch * ms / 2,
                        self.cw * ms, self.ch * ms, 14)
        self._set_title(self.titem, selected, self.cy + self.ch * ms / 2 + 16)

    def hit_test(self, px, py):
        for i in range(self.n):
            cx = self._cx(i)
            if abs(px - cx) <= self.cw / 2 and abs(py - self.cy) <= self.ch / 2:
                return i
        return None


# ── 3. Grid (Mission Control) ────────────────────────────────────────────────
class GridLayout(BaseLayout):
    def build(self):
        sw, sh = self.sw, self.sh
        self.cols = min(self.n, 4) or 1
        self.rows = math.ceil(self.n / self.cols)
        margin, gap = 150, 26
        self.tw = (sw - 2 * margin - (self.cols - 1) * gap) / self.cols
        self.th = self.tw * 0.6
        # shrink to fit height
        gh = self.rows * self.th + (self.rows - 1) * gap + 120
        if gh > sh:
            f = (sh - 120) / (self.rows * self.th + (self.rows - 1) * gap)
            self.tw *= f; self.th *= f
        self.gap = gap
        self.gw = self.cols * self.tw + (self.cols - 1) * gap
        self.gh = self.rows * self.th + (self.rows - 1) * gap
        self.ox = (sw - self.gw) / 2
        self.oy = (sh - self.gh) / 2 - 10
        self._cardwh = (self.tw, self.th, 14)
        self.pix = [_card_pix(im, self.tw, self.th, 14) for im in self.imgs]
        for i in range(self.n):
            it = self.scene.addPixmap(self.pix[i]); it.setZValue(2)
            it.setTransformOriginPoint(self.tw / 2, self.th / 2)
            self.items.append(it)
        self.ring = self._ring(3)
        self.titem = self._title()

    def _xy(self, i):
        r, c = divmod(i, self.cols)
        return self.ox + c * (self.tw + self.gap), self.oy + r * (self.th + self.gap)

    def update(self, focus, selected):
        for i in range(self.n):
            x, y = self._xy(i); it = self.items[i]
            sel = (i == selected)
            it.setScale(1.06 if sel else 1.0)
            it.setPos(x, y); it.setOpacity(1.0 if sel else 0.82)
        x, y = self._xy(selected); m = 1.06
        self._ring_rect(self.ring, x - self.tw * (m - 1) / 2, y - self.th * (m - 1) / 2,
                        self.tw * m, self.th * m, 14)
        self._set_title(self.titem, selected, self.oy + self.gh + 16)

    def hit_test(self, px, py):
        for i in range(self.n):
            x, y = self._xy(i)
            if x <= px <= x + self.tw and y <= py <= y + self.th:
                return i
        return None


# ── 4. Coverflow (3D) ────────────────────────────────────────────────────────
class CoverflowLayout(BaseLayout):
    CW, CH, STEP = 300, 190, 175

    def build(self):
        self.cy = self.sh / 2 - 20
        self._cardwh = (self.CW, self.CH, 16)
        self.pix = [_card_pix(im, self.CW, self.CH, 16) for im in self.imgs]
        for i in range(self.n):
            it = self.scene.addPixmap(self.pix[i])
            self.items.append(it)
        self.ring = self._ring(20)
        self.titem = self._title()

    def update(self, focus, selected):
        cw, ch = self.CW, self.CH
        for i in range(self.n):
            o = i - focus
            s = 1.0 if abs(o) < 0.5 else max(0.6, 0.86 - (abs(o) - 0.5) * 0.13)
            cx = self.sw / 2 + o * self.STEP
            angle = max(-60, min(60, -o * 55))
            tr = QtGui.QTransform()
            tr.translate(cx, self.cy); tr.rotate(angle, YAxis); tr.scale(s, s)
            tr.translate(-cw / 2, -ch / 2)
            it = self.items[i]
            it.setTransform(tr); it.setPos(0, 0)
            it.setZValue(20 - abs(o)); it.setOpacity(max(0.35, 1 - abs(o) * 0.28))
        # ring on focused (flat, centred)
        self._ring_rect(self.ring, self.sw / 2 - cw / 2, self.cy - ch / 2, cw, ch, 16)
        self._set_title(self.titem, selected, self.cy + ch / 2 + 26)

    def hit_test(self, px, py):
        best, bestd = None, 1e9
        for i in range(self.n):
            cx = self.sw / 2 + (i - i) * 0  # centred per round
        # simplest: nearest column to cursor x
        for i in range(self.n):
            cx = self.sw / 2 + (i - round((px - self.sw / 2) / self.STEP)) * 0
        # fall back: map x offset to index delta
        return None


# ── 5. Fan (arc) ─────────────────────────────────────────────────────────────
class FanLayout(BaseLayout):
    CW, CH, ANGLE = 240, 150, 13.0

    def build(self):
        self.cy = self.sh / 2 - 30
        self.arc = self.sw * 0.95
        self.pivot_y = self.cy + self.arc
        self._cardwh = (self.CW, self.CH, 16)
        self.pix = [_card_pix(im, self.CW, self.CH, 16) for im in self.imgs]
        for i in range(self.n):
            it = self.scene.addPixmap(self.pix[i])
            it.setTransformOriginPoint(self.CW / 2, self.CH / 2)
            self.items.append(it)
        self.ring = self._ring(20)
        self.titem = self._title()

    def update(self, focus, selected):
        cw, ch = self.CW, self.CH
        for i in range(self.n):
            o = i - focus
            ang = math.radians(o * self.ANGLE)
            cx = self.sw / 2 + self.arc * math.sin(ang)
            cyc = self.pivot_y - self.arc * math.cos(ang)
            s = 1.18 if abs(o) < 0.5 else max(0.64, 1.0 - 0.08 * abs(o))
            it = self.items[i]
            it.setScale(s); it.setRotation(math.degrees(ang))
            it.setPos(cx - cw / 2, cyc - ch / 2)
            it.setZValue(20 - abs(o)); it.setOpacity(max(0.4, 1 - abs(o) * 0.16))
        # ring on focused
        s = 1.18
        self._ring_rect(self.ring, self.sw / 2 - cw * s / 2, self.cy - ch * s / 2,
                        cw * s, ch * s, 16)
        self._set_title(self.titem, selected, self.cy + ch * s / 2 + 24)

    def hit_test(self, px, py):
        return None


# ── 6. Wallet (vertical stack) ───────────────────────────────────────────────
class WalletLayout(BaseLayout):
    def build(self):
        self.cw = int(self.sw * 0.46); self.ch = int(self.cw * 0.46)
        self.cy = self.sh / 2 - 10
        self._cardwh = (self.cw, self.ch, 18)
        self.pix = [_card_pix(im, self.cw, self.ch, 18) for im in self.imgs]
        for i in range(self.n):
            it = self.scene.addPixmap(self.pix[i])
            it.setTransformOriginPoint(self.cw / 2, self.ch / 2)
            self.items.append(it)
        self.ring = self._ring(20)
        self.titem = self._title()

    def update(self, focus, selected):
        order = sorted(range(self.n), key=lambda k: -abs(k - selected))
        for i in order:
            o = i - selected
            s = 1.0 if o == 0 else max(0.84, 0.94 - (abs(o) - 1) * 0.06)
            y = self.cy + o * self.ch * 0.42
            it = self.items[i]
            it.setScale(s); it.setPos(self.sw / 2 - self.cw / 2, y - self.ch / 2)
            it.setZValue(20 - abs(o))
            it.setOpacity(1.0 if o == 0 else max(0.45, 0.9 - abs(o) * 0.18))
        self._ring_rect(self.ring, self.sw / 2 - self.cw / 2, self.cy - self.ch / 2,
                        self.cw, self.ch, 18)
        self._set_title(self.titem, selected, self.cy + self.ch / 2 + 22)

    def hit_test(self, px, py):
        if abs(px - self.sw / 2) <= self.cw / 2 and abs(py - self.cy) <= self.ch / 2:
            return None  # center = selected handled by caller
        return None


# ── 7. Hero + Filmstrip ──────────────────────────────────────────────────────
class HeroLayout(BaseLayout):
    FW, FH, GAP = 150, 94, 16

    def build(self):
        sw, sh = self.sw, self.sh
        self.basey = sh - 36
        title_y = self.basey - self.FH - 44
        self.preview = _Preview(self.scene, self.imgs, self.windows, sw, 46,
                                title_y - 18 - 46, int(sw * 0.9), self.accent, title_y)
        self._cardwh = (self.FW, self.FH, 10)
        self.pix = [_card_pix(im, self.FW, self.FH, 10) for im in self.imgs]
        total = self.n * self.FW + (self.n - 1) * self.GAP
        self.x0 = (sw - total) / 2
        for i in range(self.n):
            it = self.scene.addPixmap(self.pix[i]); it.setZValue(2)
            it.setTransformOriginPoint(self.FW / 2, self.FH)
            self.items.append(it)
        self.ring = self._ring(3)

    def _x(self, i):
        return self.x0 + i * (self.FW + self.GAP)

    def update(self, focus, selected):
        self.preview.show(selected)
        for i in range(self.n):
            sel = (i == selected); it = self.items[i]
            it.setScale(1.15 if sel else 1.0)
            it.setPos(self._x(i), self.basey - self.FH)
            it.setOpacity(1.0 if sel else 0.7)
        x = self._x(selected); m = 1.15
        self._ring_rect(self.ring, x - self.FW * (m - 1) / 2, self.basey - self.FH * m,
                        self.FW * m, self.FH * m, 10)

    def hit_test(self, px, py):
        for i in range(self.n):
            x = self._x(i)
            if x <= px <= x + self.FW and self.basey - self.FH <= py <= self.basey:
                return i
        return None


LAYOUTS = {
    "dock": DockLayout, "row": RowLayout, "grid": GridLayout,
    "coverflow": CoverflowLayout, "fan": FanLayout, "wallet": WalletLayout,
    "hero": HeroLayout,
}
