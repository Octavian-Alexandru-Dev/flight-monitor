#!/usr/bin/env python3
"""Rebuilds the trimmed Phosphor icon font used by the web UI.

Usage: build-icons.py <phosphor regular style.css> <Phosphor.woff2>
Needs fonttools + brotli. Scans the templates for `ph-*` classes and keeps only those glyphs.
"""
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEB = ROOT / "crates" / "web"


def used_icons():
    names = set()
    for path in list((WEB / "templates").rglob("*.html")) + list((WEB / "src").rglob("*.rs")):
        names.update(re.findall(r"\bph-([a-z0-9-]+)", path.read_text()))
    return sorted(names)


def main(css_path, font_path):
    css = pathlib.Path(css_path).read_text()
    rules = dict(re.findall(r'\.ph\.ph-([a-z0-9-]+):before \{\s*content: "\\([0-9a-f]+)";', css))
    names = used_icons()
    missing = [n for n in names if n not in rules]
    if missing:
        sys.exit(f"unknown icons: {missing}")
    codepoints = ",".join(f"U+{rules[n]}" for n in names)
    subprocess.run(
        ["pyftsubset", font_path, f"--unicodes={codepoints}", "--flavor=woff2",
         f"--output-file={WEB / 'static' / 'icons.woff2'}", "--layout-features=", "--no-hinting"],
        check=True,
    )
    lines = [
        '@font-face { font-family: "Phosphor"; src: url("/static/icons.woff2") format("woff2");'
        " font-weight: normal; font-style: normal; font-display: block; }",
        '.ph { font-family: "Phosphor" !important; speak: never; font-style: normal; font-weight: normal;'
        " font-variant: normal; text-transform: none; line-height: 1; -webkit-font-smoothing: antialiased; }",
    ]
    lines += [f'.ph.ph-{n}:before {{ content: "\\{rules[n]}"; }}' for n in names]
    (WEB / "static" / "icons.css").write_text("\n".join(lines) + "\n")
    print(f"{len(names)} icons")


if __name__ == "__main__":
    main(*sys.argv[1:3])
