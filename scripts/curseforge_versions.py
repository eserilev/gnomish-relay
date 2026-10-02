"""Stops a CurseForge upload when CurseForge lacks the game version of a client in the TOC.

The BigWigs packager does not stop there: it warns and tags an older version (SPEC.md 7.9).

Usage: CF_API_KEY=<token> curseforge_versions.py <GnomishRelay.toc>
"""

import json
import os
import sys
import urllib.request
from pathlib import Path

VERSIONS_URL = "https://wow.curseforge.com/api/game/wow/versions"

# The CurseForge game type of each client, by the first two digits of its interface
# number. The packager maps them the same way (`toc_to_type` in its release.sh).
GAME_TYPES = {"16": 88568, "20": 73246}


def interfaces(toc):
    """The numbers of the `## Interface` line of a TOC."""
    for line in toc.splitlines():
        if line.startswith("## Interface:"):
            return [int(n) for n in line.split(":", 1)[1].split(",")]
    raise ValueError("the TOC has no ## Interface line")


def game_version(interface):
    """The version of an interface number: 20506 is 2.5.6."""
    return f"{interface // 10000}.{interface // 100 % 100}.{interface % 100}"


def game_type(interface):
    prefix = str(interface)[:2]
    if len(str(interface)) != 5 or prefix not in GAME_TYPES:
        raise ValueError(f"no CurseForge game type is known for the interface {interface}")
    return GAME_TYPES[prefix]


def missing(versions, toc):
    """Each game version of the TOC that CurseForge does not list for its game type."""
    known = {(v["gameVersionTypeID"], v["name"]) for v in versions}
    wanted = [(game_type(i), game_version(i)) for i in interfaces(toc)]
    return [version for kind, version in wanted if (kind, version) not in known]


def fetch_versions(token):
    request = urllib.request.Request(VERSIONS_URL, headers={"x-api-token": token})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: CF_API_KEY=<token> curseforge_versions.py <GnomishRelay.toc>")
    token = os.environ.get("CF_API_KEY", "")
    if not token:
        sys.exit("error: CF_API_KEY is not set")
    toc = Path(sys.argv[1]).read_text()
    try:
        gone = missing(fetch_versions(token), toc)
    except ValueError as error:
        sys.exit(f"error: {error}")
    if gone:
        sys.exit(f"error: CurseForge has no game version {', '.join(gone)}. Wait until it does, then run the release again.")
    print(f"CurseForge has the game version of each client: {', '.join(map(game_version, interfaces(toc)))}")


if __name__ == "__main__":
    main()
