"""A small reader for Unity's serialized YAML: scenes, prefabs, materials and
`.meta` files.

Unity writes a restricted, regular YAML: documents headed
`--- !u!<class id> &<file id>` (with ` stripped` on stubs of prefab
instances), block mappings and sequences, and one-line flow mappings such as
`{fileID: 2100000, guid: 58d5..., type: 2}`. This reader handles exactly that,
so the importer needs nothing outside the standard library.
"""

import re

HEADER = re.compile(r"^--- !u!(\d+) &(-?\d+)( stripped)?\s*$")


def parse_scalar(text):
    text = text.strip()
    if text.startswith("{") and text.endswith("}"):
        return parse_flow(text)
    if text.startswith("[") and text.endswith("]"):
        inner = text[1:-1].strip()
        return [parse_scalar(part) for part in inner.split(",")] if inner else []
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "'\"":
        return text[1:-1]
    return text


def parse_flow(text):
    """`{a: 1, b: {c: 2}}` as a dict of strings (one level of nesting)."""
    inner = text.strip()[1:-1]
    result = {}
    depth, start = 0, 0
    parts = []
    for i, ch in enumerate(inner):
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        elif ch == "," and depth == 0:
            parts.append(inner[start:i])
            start = i + 1
    parts.append(inner[start:])
    for part in parts:
        if ":" not in part:
            continue
        key, value = part.split(":", 1)
        result[key.strip()] = parse_scalar(value)
    return result


def _indent(line):
    return len(line) - len(line.lstrip(" "))


def _parse_block(lines, index, indent):
    """Parses the mapping or sequence whose lines start at `indent`."""
    if index < len(lines) and lines[index].lstrip().startswith("- ") and _indent(lines[index]) == indent:
        items = []
        while index < len(lines) and _indent(lines[index]) == indent and lines[index].lstrip().startswith("-"):
            body = lines[index].lstrip()[1:].lstrip()
            if not body:
                value, index = _parse_block(lines, index + 1, _child_indent(lines, index + 1, indent))
                items.append(value)
                continue
            if re.match(r"^[^{\[\"'][^:]*:( |$)", body):
                # A mapping item: re-read its first key at the item's inner indent.
                inner = indent + 2
                lines[index] = " " * inner + body
                value, index = _parse_mapping(lines, index, inner)
                items.append(value)
            else:
                items.append(parse_scalar(body))
                index += 1
        return items, index
    return _parse_mapping(lines, index, indent)


def _child_indent(lines, index, indent):
    if index < len(lines):
        return _indent(lines[index])
    return indent + 2


def _parse_mapping(lines, index, indent):
    result = {}
    while index < len(lines):
        line = lines[index]
        if not line.strip():
            index += 1
            continue
        current = _indent(line)
        if current < indent or (current == indent and line.lstrip().startswith("- ")):
            break
        if current > indent:
            index += 1  # continuation of a multi-line scalar
            continue
        key, _, rest = line.strip().partition(":")
        rest = rest.strip()
        if rest:
            result[key] = parse_scalar(rest)
            index += 1
        else:
            index += 1
            if index < len(lines) and (_indent(lines[index]) > indent or
                                       (_indent(lines[index]) == indent and lines[index].lstrip().startswith("- "))):
                value, index = _parse_block(lines, index, _indent(lines[index]))
                result[key] = value
            else:
                result[key] = None
    return result, index


def parse_documents(text):
    """[(class id, file id, stripped, class name, body dict)] of a Unity YAML file."""
    documents = []
    current = None
    for line in text.splitlines():
        header = HEADER.match(line)
        if header:
            current = [int(header.group(1)), int(header.group(2)), bool(header.group(3)), []]
            documents.append(current)
        elif current is not None:
            current[3].append(line)
    out = []
    for class_id, file_id, stripped, lines in documents:
        lines = [l for l in lines if not l.startswith("%")]
        if not lines:
            continue
        name = lines[0].rstrip(":").strip()
        body, _ = _parse_mapping(lines[1:], 0, 2) if len(lines) > 1 else ({}, 0)
        out.append((class_id, file_id, stripped, name, body))
    return out


def parse_meta(text):
    body, _ = _parse_mapping(text.splitlines(), 0, 0)
    return body
