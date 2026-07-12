"""
Universal Video Downloader — dark, modern, resizable GUI
Downloads from ~1,800 sites supported by yt-dlp (YouTube, Vimeo, Twitter/X,
Instagram, TikTok, Twitch, Reddit and many more), plus a generic extractor
that sniffs videos out of ordinary web pages.
================================================
Matches the approved UI design: neutral dark theme, red only on the
Download button, format card picker, inline progress bar, save-folder
picker, and download history.

Requirements:
    pip install PySide6 yt-dlp

Run:
    python universal_video_downloader.py

Build a standalone .exe (Windows):
    pip install pyinstaller
    pyinstaller --noconsole --onefile --name "Universal Video Downloader" universal_video_downloader.py
    (ffmpeg.exe should be on PATH or next to the exe for merged formats)
"""

import sys
import os
import re
import subprocess
import threading
from datetime import datetime

from PySide6.QtCore import Qt, Signal, QObject, Property, QPropertyAnimation, QEasingCurve, QByteArray
from PySide6.QtGui import QFont, QPainter, QColor, QFontMetrics, QPixmap, QIcon
from PySide6.QtSvg import QSvgRenderer
from PySide6.QtSvgWidgets import QSvgWidget
from PySide6.QtWidgets import (
    QApplication, QWidget, QMainWindow, QVBoxLayout, QHBoxLayout, QGridLayout,
    QLabel, QLineEdit, QPushButton, QFrame, QScrollArea, QFileDialog,
    QProgressBar, QSizePolicy, QCheckBox,
)

try:
    import yt_dlp
except ImportError:
    yt_dlp = None

# Where to find ffmpeg.exe (needed to merge video + audio and convert to MP4).
# - Running as a bundled .exe (PyInstaller): ffmpeg is packed inside and
#   unpacked to a temp folder at launch; we point there automatically.
# - Running as a normal .py: uses your yt-dlp folder. Edit if you move it.
if getattr(sys, "frozen", False):
    FFMPEG_DIR = getattr(sys, "_MEIPASS", os.path.dirname(sys.executable))
else:
    FFMPEG_DIR = r"C:\yt-dlp"

CREATE_NO_WINDOW = 0x08000000 if os.name == "nt" else 0

# ffmpeg binary name differs per platform
FFMPEG_NAME = "ffmpeg.exe" if os.name == "nt" else "ffmpeg"


def ffmpeg_exe():
    """Use the bundled/local ffmpeg if present, otherwise rely on PATH."""
    exe = os.path.join(FFMPEG_DIR, FFMPEG_NAME)
    return exe if os.path.isfile(exe) else "ffmpeg"

# ----------------------------------------------------------------- palette
BG        = "#141416"
PANEL     = "#1a1a1d"
FIELD     = "#1d1d21"
BORDER    = "#2e2e33"
BORDER_LO = "#26262b"
TEXT      = "#e8e8ea"
TEXT_DIM  = "#9a9aa0"
TEXT_FAINT= "#5c5c63"
GREEN     = "#7fce8a"
RED       = "#c4302b"
BLUE      = "#4d9fec"

APP_ICON_SVG = """<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" version="1.1" viewBox="0 0 256 256">
  <rect fill="#c4302b" width="256" height="256" rx="60" ry="60"/>
  <path fill="#fff" d="M95.6,78l82,50-82,50v-100Z"/>
</svg>"""

QSS = f"""
* {{ font-family: 'Segoe UI', sans-serif; }}
QMainWindow, QWidget#root {{ background: {BG}; }}
QLabel {{ color: {TEXT}; font-size: 14px; }}
QLabel[cls="section"] {{ color: {TEXT_DIM}; font-size: 11px; font-weight: 600; letter-spacing: 1px; }}
QLabel[cls="faint"]   {{ color: {TEXT_FAINT}; font-size: 12px; }}
QLabel[cls="mono"]    {{ color: {TEXT_DIM}; font-family: Consolas, monospace; font-size: 12px; }}
QLabel[cls="ok"]      {{ color: {GREEN}; font-family: Consolas, monospace; font-size: 12px; }}

QLineEdit {{
    background: {FIELD}; border: 1px solid {BORDER}; border-radius: 8px;
    padding: 10px 14px; color: {TEXT}; font-size: 14px; selection-background-color: #3a3a44;
}}
QLineEdit:focus {{ border-color: #5a5a63; background: #202024; }}

QPushButton[cls="secondary"] {{
    background: #28282d; border: 1px solid #3a3a41; border-radius: 8px;
    padding: 10px 18px; color: {TEXT}; font-size: 13px; font-weight: 600;
}}
QPushButton[cls="secondary"]:hover   {{ background: #313138; border-color: #4a4a52; }}
QPushButton[cls="secondary"]:disabled{{ color: {TEXT_FAINT}; }}

QPushButton[cls="download"] {{
    background: {RED}; border: none; border-radius: 10px;
    padding: 14px; color: white; font-size: 15px; font-weight: 700; letter-spacing: 2px;
}}
QPushButton[cls="download"]:hover    {{ background: #d13c37; }}
QPushButton[cls="download"]:disabled {{ background: #232326; color: {TEXT_FAINT}; }}

QPushButton[cls="ghost"] {{ background: none; border: none; color: {TEXT_FAINT}; font-size: 12px; }}
QPushButton[cls="ghost"]:hover {{ color: {TEXT_DIM}; }}

QPushButton[cls="convert"] {{
    background: {GREEN}; border: none; border-radius: 8px;
    padding: 10px 18px; color: #141416; font-size: 13px; font-weight: 700;
}}
QPushButton[cls="convert"]:hover    {{ background: #90dc9b; }}
QPushButton[cls="convert"]:disabled {{ background: #232326; color: {TEXT_FAINT}; }}

QPushButton[cls="cancel"] {{
    background: {RED}; border: none; border-radius: 8px;
    padding: 11px; color: white; font-size: 13px; font-weight: 700;
    letter-spacing: 1px;
}}
QPushButton[cls="cancel"]:hover    {{ background: #d8433d; }}
QPushButton[cls="cancel"]:disabled {{ background: #5a2422; color: #b09a98; }}

QFrame[cls="card"] {{
    background: {PANEL}; border: 1px solid {BORDER_LO}; border-radius: 10px;
}}
QFrame[cls="card"][selected="true"] {{ background: #1d2430; border: 1px solid {BLUE}; }}
QFrame[cls="card"]:hover {{ border-color: #5a5a63; }}

QFrame[cls="panel"] {{ background: {FIELD}; border: 1px solid {BORDER}; border-radius: 10px; }}
QFrame[cls="hist"]  {{ background: {PANEL}; border: 1px solid {BORDER_LO}; border-radius: 8px; }}

QProgressBar {{
    background: {BORDER_LO}; border: none; border-radius: 4px; height: 8px; text-align: center; color: transparent;
}}
QProgressBar::chunk {{ background: {RED}; border-radius: 4px; }}

QScrollArea {{ border: none; background: transparent; }}
QScrollBar:vertical {{ background: transparent; width: 10px; }}
QScrollBar::handle:vertical {{ background: {BORDER}; border-radius: 5px; min-height: 30px; }}
QScrollBar::add-line, QScrollBar::sub-line {{ height: 0; }}
"""


# --------------------------------------------------------- worker signals
class Bus(QObject):
    formats_ready = Signal(list, str)   # formats, error
    progress      = Signal(float, str, str)  # pct, speed, eta
    finished      = Signal(bool, str)   # ok, message
    conv_progress = Signal(float)       # pct
    conv_finished = Signal(bool, str)   # ok, message
    conv_started  = Signal()            # show conversion panel


class Switch(QCheckBox):
    """iOS-style on/off switch: gray pill when off, fully green when on."""
    TRACK_W, TRACK_H = 40, 22

    def __init__(self, text=""):
        super().__init__(text)
        self._pos = 0.0
        self.setCursor(Qt.PointingHandCursor)
        self.stateChanged.connect(self._animate)
        self._anim = QPropertyAnimation(self, b"knob", self)
        self._anim.setDuration(150)
        self._anim.setEasingCurve(QEasingCurve.OutCubic)

    def _animate(self, _state):
        self._anim.stop()
        self._anim.setEndValue(1.0 if self.isChecked() else 0.0)
        self._anim.start()

    def get_knob(self):
        return self._pos

    def set_knob(self, v):
        self._pos = v
        self.update()

    knob = Property(float, get_knob, set_knob)

    def hitButton(self, pos):
        return self.rect().contains(pos)  # whole row is clickable

    def sizeHint(self):
        fm = QFontMetrics(self.font())
        w = self.TRACK_W + 12 + fm.horizontalAdvance(self.text()) + 4
        return super().sizeHint().expandedTo(
            super().sizeHint().__class__(w, self.TRACK_H + 6))

    @staticmethod
    def _mix(c1, c2, t):
        a, b = QColor(c1), QColor(c2)
        return QColor(int(a.red() + (b.red() - a.red()) * t),
                      int(a.green() + (b.green() - a.green()) * t),
                      int(a.blue() + (b.blue() - a.blue()) * t))

    def paintEvent(self, _e):
        t = max(0.0, min(1.0, self._pos))
        p = QPainter(self)
        p.setRenderHint(QPainter.Antialiasing)
        y = (self.height() - self.TRACK_H) // 2

        # track: dark gray -> full green
        p.setPen(Qt.NoPen)
        p.setBrush(self._mix("#26262b", GREEN, t))
        p.drawRoundedRect(0, y, self.TRACK_W, self.TRACK_H,
                          self.TRACK_H / 2, self.TRACK_H / 2)

        # knob: light gray -> near-black (for contrast on green)
        pad = 3
        d = self.TRACK_H - 2 * pad
        x = pad + (self.TRACK_W - d - 2 * pad) * t
        p.setBrush(self._mix("#8a8a94", "#141416", t))
        p.drawEllipse(int(x), y + pad, d, d)

        # label
        p.setPen(QColor(TEXT if self.underMouse() else TEXT_DIM))
        p.setFont(self.font())
        p.drawText(self.TRACK_W + 12, 0,
                   self.width() - self.TRACK_W - 12, self.height(),
                   Qt.AlignVCenter | Qt.AlignLeft, self.text())


class FormatCard(QFrame):
    def __init__(self, fmt, on_select):
        super().__init__()
        self.setProperty("cls", "card")
        self.fmt = fmt
        self.on_select = on_select
        self.setCursor(Qt.PointingHandCursor)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(16, 12, 16, 12)
        lay.setSpacing(10)

        name = QLabel(fmt["label"])
        name.setStyleSheet("font-size:15px; font-weight:700; color:#f2f2f4; background:none; border:none;")
        name.setMinimumWidth(90)
        lay.addWidget(name)

        if fmt.get("best"):
            badge = QLabel("BEST")
            badge.setStyleSheet(
                "font-size:9px; font-weight:700; padding:2px 7px; border-radius:8px;"
                f"background:#2b3a2e; color:{GREEN}; border:none;")
            lay.addWidget(badge)

        for chip in fmt["chips"]:
            c = QLabel(chip)
            c.setStyleSheet(
                "font-size:11px; padding:2px 8px; border-radius:5px; border:none;"
                f"background:{BORDER_LO}; color:#a8a8ae; font-family:Consolas,monospace;")
            lay.addWidget(c)

        lay.addStretch()

        size = QLabel(fmt["size"])
        size.setStyleSheet(f"font-size:12px; color:#6e6e75; background:none; border:none;")
        lay.addWidget(size)

        self.check = QLabel("✓")
        self.check.setStyleSheet(
            f"background:{BLUE}; color:white; border-radius:9px; font-size:11px;"
            "font-weight:800; min-width:18px; max-width:18px; min-height:18px; max-height:18px;"
            "qproperty-alignment: AlignCenter; border:none;")
        self.check.hide()
        lay.addWidget(self.check)

    def set_selected(self, sel):
        self.setProperty("selected", "true" if sel else "false")
        self.check.setVisible(sel)
        self.style().unpolish(self)
        self.style().polish(self)

    def mousePressEvent(self, e):
        self.on_select(self.fmt)


class Main(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Universal Video Downloader")
        self.resize(760, 720)
        self.setMinimumSize(520, 480)

        self.bus = Bus()
        self.bus.formats_ready.connect(self.on_formats)
        self.bus.progress.connect(self.on_progress)
        self.bus.finished.connect(self.on_finished)
        self.bus.conv_progress.connect(self.on_conv_progress)
        self.bus.conv_finished.connect(self.on_conv_finished)
        self.bus.conv_started.connect(self.on_conv_started)

        self.folder = os.path.join(os.path.expanduser("~"), "Downloads")
        self.selected = None
        self.cards = []
        self._cancel_dl = False
        self._cancel_conv = False
        self._conv_proc = None

        root = QWidget(objectName="root")
        self.setCentralWidget(root)
        outer = QVBoxLayout(root)
        outer.setContentsMargins(0, 0, 0, 0)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        outer.addWidget(scroll)

        body = QWidget(objectName="root")
        scroll.setWidget(body)
        col = QVBoxLayout(body)
        col.setContentsMargins(28, 28, 28, 28)
        col.setSpacing(22)

        # App header: icon + title, centered
        head = QHBoxLayout()
        head.setSpacing(14)
        head.addStretch()
        icon = QSvgWidget()
        icon.load(QByteArray(APP_ICON_SVG.encode()))
        icon.setFixedSize(42, 42)
        head.addWidget(icon)
        title = QLabel("Universal Video Downloader")
        title.setStyleSheet("font-size:22px; font-weight:800; color:#f2f2f4; background:none; border:none;")
        head.addWidget(title)
        by = QLabel("by Kubs")
        by.setStyleSheet("font-size:12px; color:#6e6e75; background:none; border:none; padding-bottom:3px;")
        head.addWidget(by, 0, Qt.AlignBottom)
        head.addStretch()
        col.addLayout(head)

        # window / taskbar icon from the same SVG
        renderer = QSvgRenderer(QByteArray(APP_ICON_SVG.encode()))
        pm = QPixmap(64, 64)
        pm.fill(Qt.transparent)
        painter = QPainter(pm)
        renderer.render(painter)
        painter.end()
        self.setWindowIcon(QIcon(pm))

        # URL row
        col.addWidget(self._section("VIDEO LINK"))
        row = QHBoxLayout(); row.setSpacing(10)
        self.url = QLineEdit(placeholderText="Paste a video link from any site…")
        self.url.returnPressed.connect(self.fetch_formats)
        self.btn_fetch = QPushButton("Get formats"); self.btn_fetch.setProperty("cls", "secondary")
        self.btn_fetch.clicked.connect(self.fetch_formats)
        row.addWidget(self.url, 1); row.addWidget(self.btn_fetch)
        col.addLayout(row)

        # Formats
        self.fmt_header = self._section("QUALITY / FORMAT")
        self.fmt_header.hide()
        col.addWidget(self.fmt_header)
        self.fmt_grid_host = QWidget()
        self.fmt_grid = QGridLayout(self.fmt_grid_host)
        self.fmt_grid.setContentsMargins(0, 0, 0, 0)
        self.fmt_grid.setSpacing(10)
        col.addWidget(self.fmt_grid_host)

        # Save to
        col.addWidget(self._section("SAVE TO"))
        row2 = QHBoxLayout(); row2.setSpacing(10)
        self.folder_lbl = QLineEdit(self.folder); self.folder_lbl.setReadOnly(True)
        btn_browse = QPushButton("Browse…"); btn_browse.setProperty("cls", "secondary")
        btn_browse.clicked.connect(self.browse)
        row2.addWidget(self.folder_lbl, 1); row2.addWidget(btn_browse)
        col.addLayout(row2)

        self.chk_auto = Switch("Auto-convert downloads to MP4")
        self.chk_auto.setChecked(True)
        col.addWidget(self.chk_auto)

        # Download + progress
        self.btn_dl = QPushButton("DOWNLOAD"); self.btn_dl.setProperty("cls", "download")
        self.btn_dl.setEnabled(False)
        self.btn_dl.clicked.connect(self.start_download)
        col.addWidget(self.btn_dl)

        self.prog_panel = QFrame(); self.prog_panel.setProperty("cls", "panel"); self.prog_panel.hide()
        pv = QVBoxLayout(self.prog_panel); pv.setContentsMargins(16, 14, 16, 14); pv.setSpacing(8)
        ph = QHBoxLayout()
        self.prog_label = QLabel("Downloading video…")
        self.prog_label.setStyleSheet("font-size:13px; font-weight:600; border:none; background:none;")
        self.prog_pct = QLabel("0%"); self.prog_pct.setProperty("cls", "mono")
        self.prog_pct.setStyleSheet("border:none; background:none; font-family:Consolas; color:#9a9aa0;")
        ph.addWidget(self.prog_label); ph.addStretch(); ph.addWidget(self.prog_pct)
        pv.addLayout(ph)
        self.bar = QProgressBar(); self.bar.setRange(0, 1000); self.bar.setFixedHeight(8)
        pv.addWidget(self.bar)
        ph2 = QHBoxLayout()
        self.prog_speed = QLabel(""); self.prog_speed.setProperty("cls", "faint")
        self.prog_speed.setStyleSheet("border:none;background:none;font-family:Consolas;color:#6e6e75;font-size:11px;")
        self.prog_eta = QLabel(""); self.prog_eta.setStyleSheet(self.prog_speed.styleSheet())
        ph2.addWidget(self.prog_speed); ph2.addStretch(); ph2.addWidget(self.prog_eta)
        pv.addLayout(ph2)
        col.addWidget(self.prog_panel)

        self.status = QLabel(""); self.status.setProperty("cls", "ok")
        col.addWidget(self.status)

        # Convert to MP4
        col.addWidget(self._section("CONVERT TO MP4"))
        row3 = QHBoxLayout(); row3.setSpacing(10)
        self.conv_path = QLineEdit(placeholderText="Choose a file to convert…")
        self.conv_path.setReadOnly(True)
        btn_choose = QPushButton("Choose file…"); btn_choose.setProperty("cls", "secondary")
        btn_choose.clicked.connect(self.choose_convert_file)
        self.btn_conv = QPushButton("Convert"); self.btn_conv.setProperty("cls", "convert")
        self.btn_conv.setEnabled(False)
        self.btn_conv.clicked.connect(self.start_convert)
        row3.addWidget(self.conv_path, 1); row3.addWidget(btn_choose); row3.addWidget(self.btn_conv)
        col.addLayout(row3)

        self.conv_panel = QFrame(); self.conv_panel.setProperty("cls", "panel"); self.conv_panel.hide()
        cv = QVBoxLayout(self.conv_panel); cv.setContentsMargins(16, 14, 16, 14); cv.setSpacing(8)
        ch = QHBoxLayout()
        conv_lbl = QLabel("Converting to MP4…")
        conv_lbl.setStyleSheet("font-size:13px; font-weight:600; border:none; background:none;")
        self.conv_pct = QLabel("0%"); self.conv_pct.setProperty("cls", "mono")
        self.conv_pct.setStyleSheet("border:none; background:none; font-family:Consolas; color:#9a9aa0;")
        ch.addWidget(conv_lbl); ch.addStretch(); ch.addWidget(self.conv_pct)
        cv.addLayout(ch)
        self.conv_bar = QProgressBar(); self.conv_bar.setRange(0, 1000); self.conv_bar.setFixedHeight(8)
        cv.addWidget(self.conv_bar)
        col.addWidget(self.conv_panel)

        self.conv_status = QLabel(""); self.conv_status.setProperty("cls", "ok")
        col.addWidget(self.conv_status)

        # History
        hh = QHBoxLayout()
        self.hist_label = self._section("HISTORY"); self.hist_label.hide()
        self.btn_clear = QPushButton("Clear"); self.btn_clear.setProperty("cls", "ghost"); self.btn_clear.hide()
        self.btn_clear.clicked.connect(self.clear_history)
        hh.addWidget(self.hist_label); hh.addStretch(); hh.addWidget(self.btn_clear)
        col.addLayout(hh)
        self.hist_col = QVBoxLayout(); self.hist_col.setSpacing(6)
        col.addLayout(self.hist_col)

        col.addStretch()

        # Cancel bar: pinned to the bottom, above the status footer,
        # visible only while a download or conversion is running
        self.cancel_bar = QWidget(objectName="root")
        cb = QVBoxLayout(self.cancel_bar)
        cb.setContentsMargins(28, 10, 28, 12)
        self.btn_cancel = QPushButton("CANCEL")
        self.btn_cancel.setProperty("cls", "cancel")
        self.btn_cancel.setCursor(Qt.PointingHandCursor)
        self.btn_cancel.clicked.connect(self.cancel_operation)
        cb.addWidget(self.btn_cancel)
        self.cancel_bar.hide()
        outer.addWidget(self.cancel_bar)

        # Status bar
        sb = QHBoxLayout(); sb.setContentsMargins(16, 6, 16, 6)
        foot = QWidget(objectName="root")
        foot.setStyleSheet(f"background:{PANEL}; border-top:1px solid {BORDER_LO};")
        foot.setLayout(sb)
        self.foot_lbl = QLabel("idle"); self.foot_lbl.setProperty("cls", "faint")
        self.foot_lbl.setStyleSheet("font-family:Consolas; font-size:11px; color:#5c5c63; border:none; background:none;")
        sb.addWidget(self.foot_lbl); sb.addStretch()
        outer.addWidget(foot)

    # ---------------------------------------------------------- helpers
    def _section(self, text):
        lbl = QLabel(text)
        lbl.setProperty("cls", "section")
        return lbl

    def _show_cancel(self):
        self.btn_cancel.setText("CANCEL")
        self.btn_cancel.setEnabled(True)
        self.cancel_bar.show()

    def _hide_cancel(self):
        self.cancel_bar.hide()

    def cancel_operation(self):
        self.btn_cancel.setEnabled(False)
        self.btn_cancel.setText("CANCELLING…")
        self._cancel_dl = True
        self._cancel_conv = True
        proc = self._conv_proc
        if proc is not None and proc.poll() is None:
            proc.kill()

    def browse(self):
        d = QFileDialog.getExistingDirectory(self, "Choose download folder", self.folder)
        if d:
            self.folder = d
            self.folder_lbl.setText(d)

    # ---------------------------------------------------------- formats
    def fetch_formats(self):
        url = self.url.text().strip()
        if not url:
            return
        if yt_dlp is None:
            self.status.setText("yt-dlp is not installed:  pip install yt-dlp")
            return
        self.btn_fetch.setEnabled(False); self.btn_fetch.setText("Fetching…")
        self.foot_lbl.setText("querying formats…")
        threading.Thread(target=self._fetch_worker, args=(url,), daemon=True).start()

    def _fetch_worker(self, url):
        try:
            with yt_dlp.YoutubeDL({"quiet": True, "noplaylist": True}) as ydl:
                info = ydl.extract_info(url, download=False)
            fmts = self._summarize(info)
            self.bus.formats_ready.emit(fmts, "")
        except Exception as e:
            self.bus.formats_ready.emit([], str(e))

    @staticmethod
    def _est_bytes(f, duration):
        """Best-effort size for one format: exact if known, else bitrate x duration."""
        if not f:
            return 0
        size = f.get("filesize") or f.get("filesize_approx")
        if size:
            return size
        tbr = f.get("tbr") or f.get("abr") or f.get("vbr")
        if tbr and duration:
            return tbr * 1000 / 8 * duration  # kbps -> bytes
        return 0

    @staticmethod
    def _fmt_bytes(num):
        if not num:
            return "size unknown"
        mb = num / (1024 * 1024)
        if mb >= 1024:
            return f"~{mb / 1024:.2f} GB"
        if mb >= 10:
            return f"~{mb:.0f} MB"
        return f"~{mb:.1f} MB"

    def _summarize(self, info):
        """Collapse raw formats into friendly preset cards, with size estimates.

        Tolerant of non-YouTube sites: handles progressive formats (video+audio
        in one file), formats with unknown codecs, missing heights, missing
        sizes, and sites with no separate audio streams.
        """
        formats  = info.get("formats") or []
        duration = info.get("duration") or 0

        def has_video(f):
            # vcodec known, or a resolution reported, or explicitly not audio-only
            if f.get("vcodec") not in (None, "none"):
                return True
            return bool(f.get("height") or f.get("width"))

        def audio_only(f):
            return (f.get("vcodec") in (None, "none")
                    and f.get("acodec") not in (None, "none"))

        def progressive(f):
            return f.get("acodec") not in (None, "none")  # carries its own audio

        videos = [f for f in formats if has_video(f)]
        audios = [f for f in formats if audio_only(f)]

        best_audio = max(audios, key=lambda f: f.get("abr") or f.get("tbr") or 0,
                         default=None)
        audio_bytes = self._est_bytes(best_audio, duration)

        def best_at(h):
            cands = [f for f in videos if f.get("height") == h]
            return max(cands, key=lambda f: f.get("tbr") or f.get("vbr") or 0,
                       default=None)

        def total_bytes(f):
            """Size of what actually lands on disk for a chosen video format."""
            if f is None:
                return 0
            vbytes = self._est_bytes(f, duration)
            if not vbytes:
                return 0
            # progressive already includes audio; split streams need it added
            return vbytes if progressive(f) else vbytes + audio_bytes

        heights = sorted({f["height"] for f in videos if f.get("height")},
                         reverse=True)

        cards = []
        if videos:
            top = best_at(heights[0]) if heights else max(
                videos, key=lambda f: f.get("tbr") or f.get("vbr") or 0)
            cards.append({"label": "Best", "best": True,
                          "chips": ["auto", "video+audio"],
                          "size": self._fmt_bytes(total_bytes(top)),
                          "spec": "bestvideo+bestaudio/best"})

        for h in heights[:5]:
            f = best_at(h)
            ext = (f.get("ext") or "video") if f else "video"
            cards.append({"label": f"{h}p", "chips": [ext],
                          "size": self._fmt_bytes(total_bytes(f)),
                          "spec": f"bestvideo[height<={h}]+bestaudio"
                                  f"/best[height<={h}]"})

        # audio card only when the site actually offers audio-only streams
        if audios:
            m4a = [f for f in audios if f.get("ext") == "m4a"]
            pick = max(m4a, key=lambda f: f.get("abr") or f.get("tbr") or 0,
                       default=best_audio)
            cards.append({"label": "Audio only",
                          "chips": [pick.get("ext") or "audio"],
                          "size": self._fmt_bytes(self._est_bytes(pick, duration)),
                          "spec": "bestaudio[ext=m4a]/bestaudio"})

        # nothing recognizable (generic extractor with bare metadata):
        # offer a single catch-all card so the download is always possible
        if not cards:
            guess = formats[0] if formats else {}
            ext = guess.get("ext") or "auto"
            cards.append({"label": "Best available", "best": True,
                          "chips": [ext],
                          "size": self._fmt_bytes(
                              self._est_bytes(guess, duration)),
                          "spec": "best"})

        self.video_title = info.get("title", "video")
        return cards

    def on_formats(self, fmts, err):
        self.btn_fetch.setEnabled(True); self.btn_fetch.setText("Get formats")
        if err:
            self.status.setStyleSheet("color:#d8756a; font-family:Consolas; font-size:12px;")
            self.status.setText("Error: " + err[:120])
            self.foot_lbl.setText("error")
            return
        self.status.setStyleSheet("")
        self.status.setProperty("cls", "ok")
        self.status.setText(f"Found formats for: {getattr(self, 'video_title', '')}")
        self.foot_lbl.setText("ready")

        # clear old cards
        for c in self.cards:
            c.setParent(None)
        self.cards = []
        self.fmt_header.show()
        cols = 1  # list style: one format per row
        for i, f in enumerate(fmts):
            card = FormatCard(f, self.select_format)
            self.fmt_grid.addWidget(card, i // cols, i % cols)
            self.cards.append(card)
        if self.cards:
            self.select_format(fmts[0])

    def select_format(self, fmt):
        self.selected = fmt
        for c in self.cards:
            c.set_selected(c.fmt is fmt or c.fmt == fmt)
        self.btn_dl.setEnabled(True)

    # --------------------------------------------------------- download
    def start_download(self):
        if not self.selected:
            return
        self.auto_convert = self.chk_auto.isChecked()
        self._cancel_dl = False
        self._cancel_conv = False
        self._show_cancel()
        self.btn_dl.hide()
        self.prog_panel.show()
        self.bar.setValue(0)
        self.status.setText("")
        threading.Thread(target=self._dl_worker, daemon=True).start()

    def _dl_worker(self):
        def hook(d):
            if self._cancel_dl:
                raise yt_dlp.utils.DownloadCancelled("cancelled by user")
            if d["status"] == "downloading":
                total = d.get("total_bytes") or d.get("total_bytes_estimate") or 0
                done = d.get("downloaded_bytes", 0)
                pct = (done / total * 100) if total else 0
                speed = d.get("speed") or 0
                eta = d.get("eta") or 0
                self.bus.progress.emit(
                    pct,
                    f"{speed/1e6:.1f} MB/s" if speed else "",
                    f"ETA {eta}s" if eta else "")
        opts = {
            "format": self.selected["spec"],
            "outtmpl": os.path.join(self.folder, "%(title)s.%(ext)s"),
            "progress_hooks": [hook],
            "quiet": True,
            "noplaylist": True,
        }
        if os.path.isdir(FFMPEG_DIR):
            opts["ffmpeg_location"] = FFMPEG_DIR
        try:
            with yt_dlp.YoutubeDL(opts) as ydl:
                info = ydl.extract_info(self.url.text().strip(), download=True)
            filepath = None
            try:
                filepath = info["requested_downloads"][0]["filepath"]
            except (KeyError, IndexError, TypeError):
                pass
            self.bus.finished.emit(True, f"✓ Saved to {self.folder}")

            # auto-convert non-mp4 video downloads (unless cancelled)
            if (self.auto_convert and not self._cancel_dl
                    and filepath and os.path.isfile(filepath)
                    and os.path.splitext(filepath)[1].lower()
                        in (".webm", ".mkv", ".flv", ".ts", ".mov", ".avi", ".m4v", ".wmv")):
                self.bus.conv_started.emit()
                self._convert_file(filepath, delete_original=True)
        except Exception as e:
            if self._cancel_dl:
                self.bus.finished.emit(False, "✕ Download cancelled")
            else:
                self.bus.finished.emit(False, "Error: " + str(e)[:140])

    def on_progress(self, pct, speed, eta):
        self.bar.setValue(int(pct * 10))
        self.prog_pct.setText(f"{pct:.0f}%")
        self.prog_speed.setText(speed)
        self.prog_eta.setText(eta)
        self.foot_lbl.setText(f"downloading {pct:.0f}%")

    def on_finished(self, ok, msg):
        self._hide_cancel()
        self.prog_panel.hide()
        self.btn_dl.show()
        if ok:
            self.status.setStyleSheet("")
            self.status.setProperty("cls", "ok")
        elif msg.startswith("✕"):
            self.status.setStyleSheet("color:#9a9aa0; font-family:Consolas; font-size:12px;")
        else:
            self.status.setStyleSheet("color:#d8756a; font-family:Consolas; font-size:12px;")
        self.status.setText(msg)
        self.foot_lbl.setText("idle")
        if ok:
            self._add_history(getattr(self, "video_title", "video"),
                              self.selected["label"], "Done")

    def _add_history(self, title, fmt, status):
        self.hist_label.show(); self.btn_clear.show()
        item = QFrame(); item.setProperty("cls", "hist")
        h = QHBoxLayout(item); h.setContentsMargins(14, 10, 14, 10); h.setSpacing(12)
        t = QLabel(f"🎬  {title}")
        t.setStyleSheet("font-size:13px; color:#d4d4d8; border:none; background:none;")
        meta = QLabel(f"{fmt} · {datetime.now().strftime('%H:%M')}")
        meta.setStyleSheet("font-family:Consolas; font-size:11px; color:#5c5c63; border:none; background:none;")
        badge = QLabel(status)
        badge.setStyleSheet(f"font-size:11px; font-weight:600; padding:3px 9px; border-radius:9px;"
                            f"background:#233227; color:{GREEN}; border:none;")
        h.addWidget(t, 1); h.addWidget(meta); h.addWidget(badge)
        self.hist_col.insertWidget(0, item)

    def clear_history(self):
        while self.hist_col.count():
            w = self.hist_col.takeAt(0).widget()
            if w:
                w.setParent(None)
        self.hist_label.hide(); self.btn_clear.hide()

    # -------------------------------------------------------- converter
    def choose_convert_file(self):
        f, _ = QFileDialog.getOpenFileName(
            self, "Choose a video to convert", self.folder,
            "Videos (*.webm *.mkv *.avi *.mov *.flv);;All files (*.*)")
        if f:
            self.conv_path.setText(f)
            self.btn_conv.setEnabled(True)
            self.conv_status.setText("")

    def start_convert(self):
        src = self.conv_path.text().strip()
        if not src or not os.path.isfile(src):
            return
        self._cancel_conv = False
        self._show_cancel()
        self.btn_conv.setEnabled(False)
        self.conv_panel.show()
        self.conv_bar.setValue(0)
        self.conv_pct.setText("0%")
        self.conv_status.setText("")
        self.foot_lbl.setText("converting…")
        threading.Thread(target=self._convert_worker, args=(src,), daemon=True).start()

    @staticmethod
    def _hms_to_sec(h, m, s):
        return int(h) * 3600 + int(m) * 60 + float(s)

    def _convert_worker(self, src):
        self._convert_file(src, delete_original=False)

    def _convert_file(self, src, delete_original=False):
        dst = os.path.splitext(src)[0] + ".mp4"
        if os.path.exists(dst):
            base = os.path.splitext(src)[0]
            i = 1
            while os.path.exists(f"{base} ({i}).mp4"):
                i += 1
            dst = f"{base} ({i}).mp4"
        try:
            ff = ffmpeg_exe()

            # probe duration so we can report percentage
            probe = subprocess.run([ff, "-i", src], capture_output=True,
                                   text=True, creationflags=CREATE_NO_WINDOW)
            m = re.search(r"Duration:\s*(\d+):(\d+):(\d+\.?\d*)", probe.stderr)
            total = self._hms_to_sec(*m.groups()) if m else 0

            cmd = [ff, "-y", "-loglevel", "error", "-i", src,
                   "-c:v", "libx264", "-preset", "fast", "-crf", "18",
                   "-pix_fmt", "yuv420p",
                   "-c:a", "aac", "-b:a", "192k",
                   "-movflags", "+faststart",
                   "-nostats", "-progress", "pipe:1",
                   dst]
            proc = subprocess.Popen(cmd, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, text=True,
                                    bufsize=1, creationflags=CREATE_NO_WINDOW)
            self._conv_proc = proc

            # drain stderr on a side thread so ffmpeg can never stall on a full pipe
            err_lines = []
            def _drain():
                for l in proc.stderr:
                    err_lines.append(l)
            threading.Thread(target=_drain, daemon=True).start()

            last_pct = -1.0
            for line in iter(proc.stdout.readline, ""):
                line = line.strip()
                pct = None
                if line.startswith("out_time_ms="):
                    try:
                        us = int(line.split("=", 1)[1])  # microseconds despite the name
                        if us > 0 and total:
                            pct = us / 1e6 / total * 100
                    except ValueError:
                        pass
                elif line.startswith("out_time="):
                    mt = re.match(r"out_time=(\d+):(\d+):(\d+\.?\d*)", line)
                    if mt and total:
                        pct = self._hms_to_sec(*mt.groups()) / total * 100
                if pct is not None:
                    pct = min(100.0, pct)
                    if pct - last_pct >= 0.2:
                        last_pct = pct
                        self.bus.conv_progress.emit(pct)
            proc.wait()
            self._conv_proc = None
            if self._cancel_conv:
                try:
                    if os.path.exists(dst):
                        os.remove(dst)  # discard the partial mp4
                except OSError:
                    pass
                self.bus.conv_finished.emit(False, "✕ Conversion cancelled")
            elif proc.returncode == 0 and os.path.isfile(dst) and os.path.getsize(dst) > 0:
                self.bus.conv_progress.emit(100.0)
                if delete_original:
                    try:
                        os.remove(src)
                    except OSError:
                        pass
                self.bus.conv_finished.emit(True, f"✓ Converted: {os.path.basename(dst)}")
            else:
                err = "".join(err_lines).strip()
                self.bus.conv_finished.emit(False, "Conversion failed. " + err[-140:])
        except FileNotFoundError:
            self.bus.conv_finished.emit(
                False, f"ffmpeg not found. Check FFMPEG_DIR ({FFMPEG_DIR}).")
        except Exception as e:
            self.bus.conv_finished.emit(False, "Error: " + str(e)[:140])

    def on_conv_started(self):
        self._show_cancel()
        self.conv_panel.show()
        self.conv_bar.setValue(0)
        self.conv_pct.setText("0%")
        self.conv_status.setText("")
        self.foot_lbl.setText("converting…")

    def on_conv_progress(self, pct):
        self.conv_bar.setValue(int(pct * 10))
        self.conv_pct.setText(f"{pct:.0f}%")
        self.foot_lbl.setText(f"converting {pct:.0f}%")

    def on_conv_finished(self, ok, msg):
        self._hide_cancel()
        self.conv_panel.hide()
        self.btn_conv.setEnabled(True)
        if ok:
            self.conv_status.setStyleSheet("")
            self.conv_status.setProperty("cls", "ok")
        elif msg.startswith("✕"):
            self.conv_status.setStyleSheet("color:#9a9aa0; font-family:Consolas; font-size:12px;")
        else:
            self.conv_status.setStyleSheet("color:#d8756a; font-family:Consolas; font-size:12px;")
        self.conv_status.setText(msg)
        self.foot_lbl.setText("idle")


if __name__ == "__main__":
    app = QApplication(sys.argv)
    app.setStyleSheet(QSS)
    app.setFont(QFont("Segoe UI", 10))
    win = Main()
    win.show()
    sys.exit(app.exec())
