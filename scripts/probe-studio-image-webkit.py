#!/usr/bin/env python3
"""Reproduce the v0.5.9 image-preview CSS bug using Linux WebKitGTK.

    xvfb-run -a /usr/bin/python3 -B scripts/probe-studio-image-webkit.py

Requires system GTK 3 / WebKit2 4.1 introspection and xvfb; these are test tools,
not application dependencies. Reads the old tagged SFC and current preview SFC,
uses their real CSS and ready-state DOM, and decodes a synthetic 832x1216 PNG.
No Vue app, native IPC, model, network or user image is used. This isolates a
WebKit CSS regression; it is not acceptance testing of the macOS application.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
from html import escape
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import zlib


ROOT = Path(__file__).resolve().parent.parent
OLD_PATH = "src/components/studio/StudioResults.vue"
NEW_PATH = "src/components/studio/StudioImagePreview.vue"
OLD_REF = "v0.5.9"


class Element:
    def __init__(self, tag="root", attrs=()):
        self.tag, self.attrs, self.children = tag, dict(attrs), []


class TemplateParser(HTMLParser):
    def __init__(self, source):
        super().__init__()
        self.root = Element()
        self.stack = [self.root]
        self.feed(source)

    def handle_starttag(self, tag, attrs):
        element = Element(tag, attrs)
        self.stack[-1].children.append(element)
        if tag not in {"img", "input", "br", "hr", "meta", "link"}:
            self.stack.append(element)

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)
        if self.stack[-1].tag == tag:
            self.stack.pop()

    def handle_endtag(self, tag):
        for index in range(len(self.stack) - 1, 0, -1):
            if self.stack[index].tag == tag:
                del self.stack[index:]
                break

    def handle_data(self, value):
        self.stack[-1].children.append(value)


def find_class(node, name):
    if isinstance(node, Element):
        if name in node.attrs.get("class", "").split():
            return node
        for child in node.children:
            found = find_class(child, name)
            if found is not None:
                return found
    return None


def png_url():
    def chunk(name, value):
        return struct.pack("!I", len(value)) + name + value + struct.pack("!I", zlib.crc32(name + value))

    rows = b"".join(b"\0" + bytes((130, 100 + y % 80, 190)) * 832 for y in range(1216))
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack("!2I5B", 832, 1216, 8, 2, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
    return "data:image/png;base64," + base64.b64encode(data).decode()


def render_ready(node, image, *, root=False):
    if isinstance(node, str):
        return escape(node)
    # Evaluate only the current component's ready branch. Loading/error behavior
    # is covered by the Vue/browser probe, not this CSS-only engine test.
    if "preview-status" in node.attrs.get("class", "").split():
        return ""
    tag = "button" if node.tag == "vbutton" else node.tag
    attrs = {key: value for key, value in node.attrs.items()
             if key in {"class", "role", "aria-label", "aria-modal", "tabindex", "draggable", "alt"}}
    attrs["data-css-probe"] = ""
    if root:
        attrs["class"] = "n-modal " + attrs.get("class", "")
        attrs["id"] = "preview-root"
    if tag == "img":
        attrs["src"] = image
    opening = "<" + tag + "".join(f' {key}="{escape(value or "", quote=True)}"' for key, value in attrs.items()) + ">"
    if tag == "img":
        return opening
    return opening + "".join(render_ready(child, image) for child in node.children) + f"</{tag}>"


def component(source, class_name, image):
    template = re.search(r"<template>(.*)</template>\s*<style", source, re.S)
    styles = re.findall(r"<style\b[^>]*>(.*?)</style>", source, re.S)
    if template is None or not styles:
        raise ValueError("Expected a Vue SFC with template and scoped styles")
    root = find_class(TemplateParser(template.group(1)).root, class_name)
    if root is None:
        raise ValueError(f"Missing preview root .{class_name}")
    css = "\n".join(styles)
    # Vue's scoped selectors have greater specificity than .n-modal. Reproduce
    # that scope on every tested local class without changing any declarations.
    css = re.sub(r"(\.[A-Za-z_][\w-]*)(?![\w-])", r"\1[data-css-probe]", css)
    return css, render_ready(root, image, root=True)


def naive_modal_css():
    source = (ROOT / "node_modules/naive-ui/es/modal/src/styles/index.cssr.mjs").read_text()
    result = []
    for name in ("modal-body-wrapper", "modal-scroll-content", "modal"):
        match = re.search(r"cB\('" + re.escape(name) + r"',\s*`([^`]*)`", source)
        if match is None:
            raise ValueError(f"Cannot extract installed Naive UI .n-{name} styles")
        result.append(f".n-{name}{{{match.group(1)}}}")
    return "\n".join(result)


MEASURE = """JSON.stringify((() => {
  const image = document.querySelector('#preview-root img');
  const box = e => { const r = e.getBoundingClientRect(); return {x:r.x,y:r.y,width:r.width,height:r.height}; };
  const root = document.querySelector('#preview-root'), close = root.querySelector('button');
  const rect = box(image), fit = Math.min(rect.width/image.naturalWidth, rect.height/image.naturalHeight);
  const content = {x:rect.x+(rect.width-image.naturalWidth*fit)/2,y:rect.y+(rect.height-image.naturalHeight*fit)/2,
    width:image.naturalWidth*fit,height:image.naturalHeight*fit};
  return {viewport:{width:innerWidth,height:innerHeight},frame:box(root),image:rect,content,close:box(close),
    decoded:image.complete && image.naturalWidth===832 && image.naturalHeight===1216,
    visible:getComputedStyle(image).visibility==='visible',margin:getComputedStyle(root).margin};
})())"""


def within(rect, viewport):
    return (rect["width"] > 0 and rect["height"] > 0 and rect["x"] >= -1 and rect["y"] >= -1
            and rect["x"] + rect["width"] <= viewport["width"] + 1
            and rect["y"] + rect["height"] <= viewport["height"] + 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("/tmp/nai5-studio-image-webkit.json"))
    args = parser.parse_args()
    old = subprocess.run(["git", "show", f"{OLD_REF}:{OLD_PATH}"], cwd=ROOT, check=True,
                         text=True, capture_output=True).stdout
    new = (ROOT / NEW_PATH).read_text()
    image = png_url()
    variants = {"before": component(old, "image-modal", image),
                "after": component(new, "studio-image-preview", image)}
    common = (ROOT / "src/styles.css").read_text() + "\n" + naive_modal_css()
    # WebKit refuses root's sandbox setup on this CI host. The ephemeral view
    # below loads only our inline synthetic HTML under a no-network CSP.
    if os.geteuid() == 0:
        os.environ.setdefault("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1")
    import gi
    gi.require_version("Gtk", "3.0")
    gi.require_version("WebKit2", "4.1")
    from gi.repository import Gtk, GLib, WebKit2

    view = WebKit2.WebView.new_with_context(WebKit2.WebContext.new_ephemeral())
    window = Gtk.Window()
    window.add(view)
    window.show_all()
    cases = [(width, zoom, variant) for width in (1280, 390) for zoom in (1, 1.2) for variant in variants]
    results, errors = [], []
    current = None

    def fail(message):
        errors.append(str(message))
        Gtk.main_quit()
        return False

    def next_case():
        nonlocal current
        if not cases:
            Gtk.main_quit()
            return False
        current = cases.pop(0)
        width, zoom, variant = current
        window.resize(width, 900)
        css, dom = variants[variant]
        view.load_html('<html><head><meta http-equiv="Content-Security-Policy" content="default-src \'none\'; '
                       'img-src data:; style-src \'unsafe-inline\'"><style>' + common + css
                       + f"html{{zoom:{zoom}}}" + '</style></head><body><div class="n-modal-body-wrapper">'
                       '<div class="n-modal-scroll-content">' + dom + '</div></div></body></html>', "file:///tmp/")
        return False

    def measured(web, result, _):
        try:
            value = json.loads(web.evaluate_javascript_finish(result).to_string())
            width, zoom, variant = current
            results.append({"width": width, "zoom": zoom, "variant": variant, **value})
            if not value["decoded"]:
                raise AssertionError(f"Synthetic PNG did not decode: {current}")
            if variant == "before":
                if value["image"]["width"] != 0 or value["image"]["height"] != 0:
                    raise AssertionError(f"Old 0x0 WebKit regression not reproduced: {current}")
            elif not (value["visible"] and within(value["frame"], value["viewport"])
                      and within(value["content"], value["viewport"]) and within(value["close"], value["viewport"])):
                raise AssertionError(f"Current preview image/frame/close is outside the viewport: {current}")
        except Exception as exc:
            return fail(exc)
        GLib.idle_add(next_case)

    def measure():
        view.evaluate_javascript(MEASURE, -1, None, None, None, measured, None)
        return False

    view.connect("load-changed", lambda _, event: GLib.timeout_add(150, measure)
                 if event == WebKit2.LoadEvent.FINISHED else None)
    GLib.idle_add(next_case)
    timeout = GLib.timeout_add_seconds(30, lambda: fail("WebKit probe timed out"))
    Gtk.main()
    GLib.source_remove(timeout)
    window.destroy()
    report = {"engine": f"WebKitGTK {WebKit2.get_major_version()}.{WebKit2.get_minor_version()}.{WebKit2.get_micro_version()}",
              "scope": "Real SFC ready-state DOM/CSS in Linux WebKitGTK; macOS application acceptance not performed",
              "sources": {"before": f"{OLD_REF}:{OLD_PATH}", "after": NEW_PATH,
                          "before_sha256": hashlib.sha256(old.encode()).hexdigest(),
                          "after_sha256": hashlib.sha256(new.encode()).hexdigest()},
              "measurements": results, "errors": errors, "passed": len(results) == 8 and not errors}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f"{'PASS' if report['passed'] else 'FAIL'}: {args.output}", file=sys.stderr)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
