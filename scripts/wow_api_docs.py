"""Reads the generated API docs of the client (Blizzard_APIDocumentationGenerated).

Each doc file is one Lua table. For each function and event, the gate keeps the
arguments, the returns, the payload, and every flag, such as `SecretArguments`
or `SecretWhenUnitThreatValuesRestricted`. It drops only names and prose.
"""

import re
from pathlib import Path

TOKEN = re.compile(r'\s+|--[^\n]*|"((?:[^"\\]|\\[\s\S])*)"|(-?\d+(?:\.\d+)?)|([A-Za-z_][\w.]*)|([{}=,;]|[-+*/%()])')
KEYWORDS = {"true": True, "false": False, "nil": None}
FIELD_LISTS = ("Arguments", "Returns", "Payload")
PROSE = {"Name", "Type", "LiteralName", "Documentation"}


class Ident(str):
    """A bare Lua name or expression in a doc, such as `Enum.SecretAspect.Text`."""


def tokens(text):
    out = []
    for match in TOKEN.finditer(text):
        string, number, name, symbol = match.groups()
        if string is not None:
            out.append(("value", string.replace('\\"', '"')))
        elif number is not None:
            out.append(("value", float(number) if "." in number else int(number)))
        elif name is not None:
            out.append(("value", KEYWORDS[name] if name in KEYWORDS else Ident(name)))
        elif symbol is not None:
            out.append((symbol, None))
    return out


def parse_value(toks, i):
    if toks[i][0] == "{":
        return parse_table(toks, i + 1)
    end = i
    while toks[end][0] not in (",", ";", "}"):
        end += 1
    if end == i + 1 and toks[i][0] == "value":
        return toks[i][1], end
    return Ident(" ".join(str(value if kind == "value" else kind) for kind, value in toks[i:end])), end


def parse_table(toks, i):
    """A table with keys becomes a dict, a table without keys a list."""
    keyed, listed = {}, []
    while toks[i][0] != "}":
        if toks[i + 1][0] == "=":
            keyed[toks[i][1]], i = parse_value(toks, i + 2)
        else:
            item, i = parse_value(toks, i)
            listed.append(item)
        if toks[i][0] in (",", ";"):
            i += 1
    if keyed and listed:
        raise ValueError("an API doc table mixes keys and list items")
    return (keyed or listed), i + 1


def parse_doc(text):
    """The table of `local Name = { ... };`. The file ends with a call that registers it."""
    toks = tokens(text)
    start = next(i for i, (kind, _) in enumerate(toks) if kind == "{")
    table, _ = parse_table(toks, start + 1)
    return table


def strip_prose(value):
    if isinstance(value, dict):
        return {k: strip_prose(v) for k, v in value.items() if k != "Documentation"}
    if isinstance(value, list):
        return [strip_prose(v) for v in value]
    return value


def signature(entry):
    return {k: strip_prose(v) for k, v in entry.items() if k not in PROSE}


def function_key(doc, function):
    """`Screenshot`, `C_Timer.After`, or `SimpleFrameAPI:RegisterEvent` for a widget method."""
    if doc.get("Type") == "ScriptObject":
        return f"{doc['Name']}:{function['Name']}"
    if doc.get("Namespace"):
        return f"{doc['Namespace']}.{function['Name']}"
    return function["Name"]


def add_unique(table, key, value, what):
    if key in table and table[key] != value:
        raise ValueError(f"the API docs list {what} {key} twice, with different entries")
    table[key] = value


class Docs:
    def __init__(self):
        self.functions = {}
        self.events = {}

    def add(self, doc):
        for function in doc.get("Functions", []):
            add_unique(self.functions, function_key(doc, function), signature(function), "function")
        for event in doc.get("Events", []):
            add_unique(self.events, event["LiteralName"], signature(event), "event")

    def methods_named(self, name):
        return sorted(key for key in self.functions if key.endswith(f":{name}"))


def read_docs(folder):
    docs = Docs()
    for path in sorted(Path(folder).glob("*.lua")):
        docs.add(parse_doc(path.read_text(encoding="utf-8")))
    return docs


def lua_key(key):
    return key if re.fullmatch(r"[A-Za-z_]\w*", key) else f'["{key}"]'


def lua_inline(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, Ident) or isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, str):
        return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{lua_key(k)} = {lua_inline(v)}" for k, v in value.items()) + " }"
    if not value:
        return "{}"
    return "{ " + ", ".join(lua_inline(v) for v in value) + " }"


def lua_entry(key, entry, indent):
    """One flag per line and one field per line, so a diff names the exact change."""
    if not entry:
        return f"{indent}{lua_key(key)} = {{}},\n"
    inner = indent + "\t"
    out = [f"{indent}{lua_key(key)} = {{\n"]
    for flag in sorted(k for k in entry if k not in FIELD_LISTS):
        out.append(f"{inner}{lua_key(flag)} = {lua_inline(entry[flag])},\n")
    for name in FIELD_LISTS:
        if name in entry:
            out.append(lua_fields(name, entry[name], inner))
    out.append(f"{indent}}},\n")
    return "".join(out)


def lua_fields(name, fields, indent):
    if not fields:
        return f"{indent}{name} = {{}},\n"
    lines = "".join(f"{indent}\t{lua_inline(field)},\n" for field in fields)
    return f"{indent}{name} = {{\n{lines}{indent}}},\n"
