#!/usr/bin/env python3
"""Feedback-loop validator for fkdisp conversion output.

Exits non-zero (RED) if any check fails. Usage: validate_xlsx.py <file.xlsx>
Checks:
  1. ZIP integrity
  2. Every XML part well-formed
  3. No _xlfn.DISPIMG formulas left in worksheet XML  (user symptom #2)
  4. No dangling r:id refs in sheet XML / drawing rels / blip embeds (repair trigger)
  5. Every drawing part declared in [Content_Types].xml
  6. Every image relationship target exists in the package
"""
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

NS_R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
path = sys.argv[1]
errors = []

z = zipfile.ZipFile(path)
bad = z.testzip()
if bad:
    errors.append(f"ZIP corrupt entry: {bad}")

xml_parts = [n for n in z.namelist() if n.endswith((".xml", ".rels"))]
for name in xml_parts:
    try:
        ET.fromstring(z.read(name))
    except ET.ParseError as e:
        errors.append(f"XML malformed: {name}: {e}")

# 3. leftover DISPIMG
for name in z.namelist():
    if re.match(r"xl/worksheets/sheet\d+\.xml$", name):
        c = z.read(name).decode("utf-8", "replace")
        n = c.count("_xlfn.DISPIMG")
        if n:
            errors.append(f"LEFTOVER DISPIMG: {name}: {n} occurrences")
total_dispimg = 0

# 4-6. relationship resolution
names = set(z.namelist())
for name in names:
    if re.match(r"xl/worksheets/sheet\d+\.xml$", name):
        c = z.read(name).decode("utf-8", "replace")
        rels_name = f"xl/worksheets/_rels/{name.split('/')[-1]}.rels"
        rels = z.read(rels_name).decode("utf-8") if rels_name in names else ""
        rid_targets = dict(re.findall(r'Id="(rId\d+)"[^>]*Target="([^"]+)"', rels))
        for rid in re.findall(r'<drawing r:id="(rId\d+)"/>', c):
            if rid not in rid_targets:
                errors.append(f"DANGLING drawing ref: {name}: {rid}")
            else:
                tgt = "xl/" + rid_targets[rid].replace("../", "")
                if tgt not in names:
                    errors.append(f"MISSING drawing part: {name}: {rid} -> {tgt}")
    if re.match(r"xl/drawings/drawing\d+\.xml$", name):
        rels_name = f"xl/drawings/_rels/{name.split('/')[-1]}.rels"
        rels = z.read(rels_name).decode("utf-8") if rels_name in names else ""
        rid_targets = dict(re.findall(r'Id="(rId\d+)"[^>]*Target="([^"]+)"', rels))
        c = z.read(name).decode("utf-8", "replace")
        for rid in re.findall(r'r:embed="(rId\d+)"', c):
            if rid not in rid_targets:
                errors.append(f"DANGLING blip embed: {name}: {rid}")
            else:
                tgt = "xl/" + rid_targets[rid].replace("../", "")
                if tgt not in names:
                    errors.append(f"MISSING image part: {name}: {rid} -> {tgt}")

# 5. content types
ct = z.read("[Content_Types].xml").decode("utf-8", "replace")
for name in names:
    if re.match(r"xl/drawings/drawing\d+\.xml$", name):
        if f'PartName="/{name}"' not in ct:
            errors.append(f"UNDECLARED content type: {name}")

if errors:
    print(f"RED: {path}: {len(errors)} problem(s)")
    for e in errors:
        print("  -", e)
    sys.exit(1)
print(f"GREEN: {path}: all checks passed ({len(xml_parts)} XML parts)")
