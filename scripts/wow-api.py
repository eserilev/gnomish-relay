"""Writes the WoW Forever API that an addon uses as two Lua tables.

Usage: wow-api.py --ui <wow-ui-source> --bir <BlizzardInterfaceResources> --build <build>
       --addon <folder> [--addon <folder>]... [--lint <wow.yml>] [--fake <wow.lua>]
       --api <api.lua> --signatures <api-signatures.lua>

The api file is for the fake game and the lint check. The signatures file keeps the
documented arguments, returns, payloads, and flags of each used function and event.

Sources:
- BlizzardInterfaceResources: the global functions, frames, and widget methods that
  the client reports about itself.
- wow-ui-source: Blizzard's own UI code. It gives the Lua globals (SOUNDKIT, fonts),
  and the templates with their mixins and child keys.

It checks every WoW name that the addon, wow.yml, or the fake game uses. A name that
the client does not have, or has only in a Blizzard_Deprecated addon, is an error.
So is an event that the client does not have.
Only the used names, the widget types, and the used templates go into the output.
"""

import argparse
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

import wow_api_docs


def arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ui", type=Path, required=True)
    parser.add_argument("--bir", type=Path, required=True)
    parser.add_argument("--build", required=True)
    parser.add_argument("--addon", type=Path, action="append", required=True)
    parser.add_argument("--lint", type=Path)
    parser.add_argument("--fake", type=Path)
    parser.add_argument("--api", type=Path, required=True)
    parser.add_argument("--signatures", type=Path, required=True)
    return parser.parse_args()


args = arguments()
build = args.build
ui = args.ui / "Interface" / "AddOns"
resources = args.bir / "Resources"


def quoted(path):
    return re.findall(r'"([^"]+)"', path.read_text(encoding="utf-8"))


# The resource list says that Region inherits Region. The widget hierarchy on
# warcraft.wiki.gg (Widget API) gives ScriptRegion.
SELF_PARENT = {"Region": "ScriptRegion"}


def widgets():
    text = (resources / "WidgetAPI.lua").read_text(encoding="utf-8")
    out = {}
    for block in re.split(r"^\t(?=\w+ = \{$)", text, flags=re.M)[1:]:
        name = block.split(" ", 1)[0]
        inherits = re.search(r"^\t\tinherits = \{([^}]*)\}", block, re.M)
        methods = re.search(r"^\t\tmethods = \{(.*?)^\t\t\}", block, re.M | re.S)
        out[name] = (
            [SELF_PARENT.get(p, p) if p == name else p for p in re.findall(r'"(\w+)"', inherits.group(1))]
            if inherits
            else [],
            re.findall(r'"(\w+)"', methods.group(1)) if methods else [],
        )
    for name, (parents, _) in out.items():
        if name in parents:
            sys.exit(f"error: widget {name} inherits itself in the resource list")
    return out


def lua_files():
    return sorted(ui.rglob("*.lua"))


def xml_files():
    return sorted(ui.rglob("*.xml"))


def ui_globals():
    names = set()
    for path in lua_files():
        text = path.read_text(encoding="utf-8", errors="replace")
        names.update(re.findall(r"^(?:_G\.)?([A-Za-z_]\w*)\s*=[^=]", text, re.M))
        names.update(re.findall(r"^function ([A-Za-z_]\w*)\s*\(", text, re.M))
        if path.name == "SoundKitConstants.lua":
            names.update("SOUNDKIT." + n for n in re.findall(r"^\t(\w+) = \d+", text, re.M))
    for path in xml_files():
        text = path.read_text(encoding="utf-8", errors="replace")
        names.update(re.findall(r'<(?:Font|FontFamily) name="(\w+)"', text))
    return names


def deprecated():
    """Globals that Blizzard keeps only in its compatibility addons. They go away later."""
    names = set()
    for folder in ui.glob("*Deprecated*"):
        for path in folder.rglob("*.lua"):
            text = path.read_text(encoding="utf-8", errors="replace")
            names.update(re.findall(r"^([A-Za-z_]\w*)\s*=[^=]", text, re.M))
            names.update(re.findall(r"^function ([A-Za-z_][\w.]*)\s*\(", text, re.M))
            names.update(re.findall(r"^\t*([A-Za-z_]\w*\.[A-Za-z_]\w*)\s*=[^=]", text, re.M))
    return names


def mixins():
    bases, methods = {}, {}
    for path in lua_files():
        text = path.read_text(encoding="utf-8", errors="replace")
        for name, args in re.findall(r"^(\w+)\s*=\s*CreateFromMixins\(([^)]*)\)", text, re.M):
            bases.setdefault(name, []).extend(re.findall(r"\w+", args))
        for name, method in re.findall(r"^function (\w+):(\w+)\s*\(", text, re.M):
            methods.setdefault(name, set()).add(method)
    return bases, methods


def mixin_methods(name, bases, methods, seen=None):
    seen = seen if seen is not None else set()
    if name in seen:
        return set()
    seen.add(name)
    out = set(methods.get(name, ()))
    for base in bases.get(name, ()):
        out |= mixin_methods(base, bases, methods, seen)
    return out


def local(tag):
    return tag.rsplit("}", 1)[-1]


def child_keys(element):
    """The parentKey of each child region. A keyed child owns what is inside it."""
    keys = set()
    for child in element:
        key = child.get("parentKey")
        if key:
            keys.add(key)
        else:
            keys |= child_keys(child)
    return keys


def templates():
    out = {}
    for path in xml_files():
        try:
            tree = ET.parse(path)
        except ET.ParseError:
            continue
        for element in tree.getroot().iter():
            name = element.get("name")
            if not name or not (element.get("virtual") == "true" or element.get("intrinsic") == "true"):
                continue
            split = lambda key: [s.strip() for s in (element.get(key) or "").split(",") if s.strip()]
            out[name] = {
                "base": local(element.tag),
                "inherits": split("inherits"),
                "mixins": split("mixin") + split("secureMixin"),
                "keys": child_keys(element),
            }
    return out


def template_names(name, all_templates, bases, methods, seen=None):
    seen = seen if seen is not None else set()
    template = all_templates.get(name)
    if template is None or name in seen:
        return set()
    seen.add(name)
    out = set(template["keys"])
    for mixin in template["mixins"]:
        out |= mixin_methods(mixin, bases, methods)
    for parent in template["inherits"]:
        out |= template_names(parent, all_templates, bases, methods, seen)
    return out


def addon_files():
    return sorted(path for folder in args.addon for path in folder.glob("*.lua"))


def addon_names():
    """A folder with a TOC is an addon. A folder without one is shared code, like addon/transport."""
    return sorted(folder.name for folder in args.addon if any(folder.glob("*.toc")))


def used_templates():
    names = set()
    for path in addon_files():
        text = path.read_text(encoding="utf-8")
        names.update(re.findall(r'"(\w+Template)"', text))
        names.update(re.findall(r'CreateFrame\("(\w+)"', text))
    return names


# Lua 5.1 itself.
LUA = set("assert error ipairs next pairs pcall print rawget rawset select setmetatable getmetatable "
          "tonumber tostring type unpack xpcall loadstring string table math os coroutine".split())


def own_prefixes():
    """The globals of the addon itself, such as `GnomishRelay_SlotData` and `SLASH_GNOMISHRELAY1`."""
    names = addon_names()
    return tuple(names + ["SLASH_" + name.upper() for name in names])


def lint_globals(path):
    """The top-level globals of wow.yml, and the fields of each struct as `name.field`."""
    text = path.read_text(encoding="utf-8")
    body = text.split("\nglobals:\n", 1)[1].split("\nstructs:\n", 1)
    names = set(re.findall(r"^  (\w+):", body[0], re.M))
    structs = {}
    if len(body) > 1:
        current = None
        for line in body[1].splitlines():
            head = re.match(r"^  (\w+):", line)
            field = re.match(r"^    (\w+):", line)
            if head:
                current = structs.setdefault(head.group(1), [])
            elif field and current is not None:
                current.append(field.group(1))
    for owner, struct in re.findall(r"^  (\w+):\n    struct: (\w+)", body[0], re.M):
        names.update(f"{owner}.{f}" for f in structs.get(struct, ()))
    return names


def referenced():
    """Every WoW name that the addon, the lint list, or the fake game uses."""
    names = lint_globals(args.lint) if args.lint else set()
    for path in addon_files():
        text = path.read_text(encoding="utf-8")
        names.update(".".join(m) for m in re.findall(r"\b(C_\w+|SOUNDKIT|bit)\.([A-Za-z_]\w*)", text))
    if args.fake:
        names |= fake_globals(args.fake)
    own = own_prefixes()
    return {n for n in names if n.split(".")[0] not in LUA | {"wow"} and not n.startswith(own)}


def fake_globals(path):
    fake = path.read_text(encoding="utf-8")
    names = set(re.findall(r"^function ([A-Za-z_]\w*(?:\.\w+)?)\s*\(", fake, re.M))
    names.update(re.findall(r"^([A-Za-z_]\w*)\s*=[^=]", fake, re.M))
    for pair in re.findall(r"^([A-Za-z_]\w*), ([A-Za-z_]\w*)\s*=[^=]", fake, re.M):
        names.update(pair)
    return names


def used_events():
    names = set()
    for path in addon_files():
        text = path.read_text(encoding="utf-8")
        names.update(re.findall(r'Register(?:Unit)?Event\("(\w+)"', text))
    return names


def used_methods():
    """Every `:Name(` call. Most are widget methods. The rest match no doc and drop out."""
    names = set()
    for path in addon_files():
        names.update(re.findall(r":([A-Za-z_]\w*)\s*\(", path.read_text(encoding="utf-8")))
    return names


def lua_list(names, indent):
    return "".join(f'{indent}"{n}",\n' for n in sorted(names))


def check_names(used, known, events, known_events):
    errors = [f"{name} is not in the WoW Forever {build} client" for name in sorted(used - known)]
    errors += [f"{name} is deprecated in WoW Forever {build}" for name in sorted(used & deprecated())]
    errors += [f"event {name} is not in the WoW Forever {build} client" for name in sorted(events - known_events)]
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    if errors:
        sys.exit(1)


def api_text(used):
    bases, methods = mixins()
    all_templates = templates()
    widget_types = widgets()
    out = [
        f"-- The WoW Forever {build} API that {', '.join(addon_names())} uses.\n",
        "-- Written by scripts/wow-api.sh. Do not edit.\n",
        "return {\n",
        f'\tbuild = "{build}",\n',
        "\tglobals = {\n",
        lua_list(used, "\t\t"),
        "\t},\n",
        "\twidgets = {\n",
    ]
    for name, (inherits, widget_methods) in sorted(widget_types.items()):
        out.append(f"\t\t{name} = {{\n\t\t\tinherits = {{\n")
        out.append(lua_list(inherits, "\t\t\t\t"))
        out.append("\t\t\t},\n\t\t\tmethods = {\n")
        out.append(lua_list(widget_methods, "\t\t\t\t"))
        out.append("\t\t\t},\n\t\t},\n")
    out.append("\t},\n\ttemplates = {\n")
    for name in sorted(used_templates() - set(widget_types)):
        template = all_templates.get(name)
        if template is None:
            sys.exit(f"error: template {name} is not in the WoW Forever {build} client")
        out.append(f'\t\t{name} = {{\n\t\t\tbase = "{template["base"]}",\n\t\t\tnames = {{\n')
        out.append(lua_list(template_names(name, all_templates, bases, methods), "\t\t\t\t"))
        out.append("\t\t\t},\n\t\t},\n")
    out.append("\t},\n}\n")
    return "".join(out)


def lua_entries(table, keys):
    return "".join(wow_api_docs.lua_entry(key, table[key], "\t\t") for key in sorted(keys))


def signatures_text(used, events, docs, global_api):
    functions = {n for n in used if n in docs.functions}
    undocumented = {n for n in used if n in global_api or n.startswith("C_") and "." in n} - functions
    methods = {key for name in used_methods() for key in docs.methods_named(name)}
    documented_events = events & set(docs.events)
    return "".join([
        f"-- The documented WoW Forever {build} API that {', '.join(addon_names())} uses.\n",
        "-- Written by scripts/wow-api.sh from Blizzard_APIDocumentationGenerated. Do not edit.\n",
        "-- A patch can change the arguments, returns, or secret flags and keep the name. The diff shows it.\n",
        "-- The scan does not know the type of each object, so methods has each widget type with a called name.\n",
        "return {\n",
        f'\tbuild = "{build}",\n',
        "\tfunctions = {\n",
        lua_entries(docs.functions, functions),
        "\t},\n\tmethods = {\n",
        lua_entries(docs.functions, methods),
        "\t},\n\tevents = {\n",
        lua_entries(docs.events, documented_events),
        "\t},\n\tundocumented = {\n",
        lua_list(undocumented | (events - documented_events), "\t\t"),
        "\t},\n}\n",
    ])


def main():
    global_api = set(quoted(resources / "GlobalAPI.lua"))
    known = global_api | set(quoted(resources / "FrameXML.lua"))
    known |= set(quoted(resources / "Frames.lua"))
    known |= ui_globals()
    known |= {n.split(".")[0] for n in known if "." in n}
    docs = wow_api_docs.read_docs(ui / "Blizzard_APIDocumentationGenerated")
    used = referenced()
    events = used_events()
    check_names(used, known, events, set(quoted(resources / "Events.lua")) | set(docs.events))
    api = api_text(used)
    signatures = signatures_text(used, events, docs, global_api)
    args.api.write_text(api, encoding="utf-8")
    args.signatures.write_text(signatures, encoding="utf-8")


main()
