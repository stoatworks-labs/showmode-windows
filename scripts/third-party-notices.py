#!/usr/bin/env python3
"""Write THIRD-PARTY-NOTICES.txt: the licence texts ShowMode.exe owes.

    scripts/third-party-notices.py            # (re)write THIRD-PARTY-NOTICES.txt
    scripts/third-party-notices.py --check    # exit 1 if it is out of date

windows-rs and serde are compiled into the executable, and MIT and Apache-2.0 both
ask for their notices to travel with the binary. So this reads the real build
rather than a hand-kept list (the same approach as amber's script):

  - The crate set is `cargo tree` over normal, non-proc-macro edges for each
    release target. Build scripts and proc macros run at compile time and ship
    nothing.
  - Each crate's licence files come from its own package. A crate that carries no
    file gets the standard text for the licence its Cargo.toml declares.

Needs cargo, and the network the first time (cargo fetches the Windows-only crates
to read their manifests). Run it after any change to Cargo.lock; scripts/release.sh
refuses to package while it is out of date.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD-PARTY-NOTICES.txt"

TARGETS = ("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")

LICENCE_FILE = re.compile(
    r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE|COPYRIGHT)([-_.].*)?$", re.I)

RULE = "=" * 78
THIN = "-" * 78

MIT_TEXT = """\
Copyright (c) {holder}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""

APACHE_START = re.compile(r"Apache License\s+Version 2\.0, January 2004")
APACHE_END = "END OF TERMS AND CONDITIONS"


def run(*cmd):
    r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"{' '.join(cmd[:3])} failed:\n{r.stderr.strip()}")
    return r.stdout


def shipped(target):
    """(name, version) of every crate linked into the executable for one target."""
    out = run("cargo", "tree", "--locked", "--target", target,
              "-e", "normal,no-proc-macro", "--prefix", "none", "--format", "{p}")
    crates = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].startswith("v"):
            crates.add((parts[0], parts[1][1:]))
    return crates


def tidy(text):
    lines = [l.rstrip() for l in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    return "\n".join(lines).strip("\n")


def licence_files(directory):
    return sorted(p for p in directory.iterdir() if p.is_file() and LICENCE_FILE.match(p.name))


def holder(authors):
    names = [re.sub(r"\s*<[^>]*>", "", a).strip() for a in authors]
    return ", ".join(n for n in names if n) or "the authors"


def split_apache(text):
    """(terms, notices) for a plain copy of the Apache-2.0 licence, else None.

    Crates ship the same licence with different wrapping, with or without the
    appendix. One copy of the terms covers them all; a filled-in copyright line is
    a notice in its own right and is kept with the crate.
    """
    m = APACHE_START.search(text)
    end = text.find(APACHE_END)
    if not m or end < m.start() or text[:m.start()].strip():
        return None
    terms = text[m.start():end + len(APACHE_END)]
    tail = text[end + len(APACHE_END):]
    notices = [l.strip() for l in tail.splitlines()
               if re.search(r"\bCopyright\b", l) and not re.search(r"[\[{]yyyy[\]}]", l)]
    rest = " ".join(" ".join(l for l in tail.splitlines() if l.strip() not in notices).split())
    if rest and not (rest.startswith("APPENDIX: How to apply the Apache License")
                     and rest.endswith("limitations under the License.") and len(rest) < 2000):
        return None
    return terms, notices


def texts_for(pkg):
    """[(label, text)] for one package, and whether it fell back to a standard text."""
    manifest_dir = Path(pkg["manifest_path"]).parent
    found = licence_files(manifest_dir)
    if pkg.get("license_file"):
        extra = (manifest_dir / pkg["license_file"]).resolve()
        if extra.is_file() and extra not in found:
            found.append(extra)
    if found:
        return [(p.name, tidy(p.read_text(encoding="utf-8", errors="replace")))
                for p in found], False
    expr = pkg.get("license") or ""
    ids = set(re.findall(r"[A-Za-z0-9.+-]+", expr))
    if "MIT" in ids:
        return [("MIT (standard text)", MIT_TEXT.format(holder=holder(pkg["authors"])))], True
    if "Apache-2.0" in ids:
        return [("Apache-2.0 (standard text)", None)], True
    sys.exit(f"{pkg['name']} {pkg['version']}: no licence file and no fallback for {expr!r}")


def block(text):
    return f"{THIN}\n{text}\n{THIN}\n"


def render():
    per_target = {t: shipped(t) for t in TARGETS}
    every = set().union(*per_target.values())
    meta = json.loads(run("cargo", "metadata", "--locked", "--format-version", "1"))
    packages = {(p["name"], p["version"]): p for p in meta["packages"]}
    ours = {(p["name"], p["version"]) for p in meta["packages"]
            if p["id"] in meta["workspace_members"]}

    crates = []
    for key in sorted(every - ours):
        pkg = packages.get(key)
        if pkg is None:
            sys.exit(f"{key[0]} {key[1]} is in the tree but not in cargo metadata")
        crates.append(pkg)

    def where(pkg):
        key = (pkg["name"], pkg["version"])
        only = [t for t in TARGETS if key in per_target[t]]
        if len(only) == len(TARGETS):
            return ""
        return " (x64 only)" if only == ["x86_64-pc-windows-msvc"] else " (ARM64 only)"

    # Group the texts, so the MIT text several crates share and the Apache-2.0
    # terms each appear once rather than once per crate. Whitespace never decides
    # a group; a word or a copyright line does.
    groups, fallback, pending_apache = {}, set(), []

    def add(key, text, label):
        group = groups.setdefault(key, {"text": text, "users": []})
        if label not in group["users"]:
            group["users"].append(label)

    for pkg in crates:
        label = f"{pkg['name']} {pkg['version']}"
        texts, fell_back = texts_for(pkg)
        if fell_back:
            fallback.add(label)
        for _, text in texts:
            if text is None:
                pending_apache.append(label)
                continue
            apache = split_apache(text)
            if apache:
                terms, notices = apache
                add(("apache", " ".join(terms.replace("https://", "http://").split())), terms,
                    label + "".join(f" ({n})" for n in notices))
            else:
                add(("text", " ".join(text.split())), text, label)
    if pending_apache:
        apaches = [k for k in groups if k[0] == "apache"]
        if not apaches:
            sys.exit("no crate ships the Apache-2.0 terms to stand in for a missing file")
        commonest = max(apaches, key=lambda k: len(groups[k]["users"]))
        for label in pending_apache:
            add(commonest, None, label)

    out = [f"""\
THIRD-PARTY NOTICES: Show Mode for Windows

Show Mode's own code is MIT licensed; see LICENSE.txt. ShowMode.exe also contains
other people's work, compiled in: the Rust crates below, and Microsoft's C runtime,
linked statically from the MSVC toolset so the PC needs no Visual C++
redistributable. This file carries the crates' copyright notices and licence texts.

Generated by scripts/third-party-notices.py from Cargo.lock. Edit that and re-run
it, not this file.

Rust crates linked into ShowMode.exe. A crate marked * ships no licence file in
its package, so the standard text of the licence its Cargo.toml declares is given
instead, with its authors as the copyright holder.

"""]
    width = max(len(f"{p['name']} {p['version']}") for p in crates) + 2
    for pkg in crates:
        label = f"{pkg['name']} {pkg['version']}"
        mark = "*" if label in fallback else " "
        out.append(f" {mark}{label:<{width}}{pkg.get('license') or '?'}{where(pkg)}\n")
    out.append(f"""
The licence texts, each after the crates it covers. Where a crate offers a choice
of licences and ships each text, all of them are given. The Apache-2.0 terms
appear once for every crate that ships a plain copy of them.

{RULE}

""")
    for group in sorted(groups.values(), key=lambda g: g["users"][0]):
        out.append(f"Used by: {', '.join(group['users'])}\n{block(group['text'])}\n")
    return "".join(out).rstrip("\n") + "\n"


def main():
    text = render()
    if "--check" in sys.argv:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != text:
            print("THIRD-PARTY-NOTICES.txt is out of date: run scripts/third-party-notices.py")
            return 1
        print("THIRD-PARTY-NOTICES.txt is current")
        return 0
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)} ({len(text) // 1024} KB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
