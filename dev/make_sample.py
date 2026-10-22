"""Builds a multi-character sample AltLedger.lua for UI development, by cloning
the simulated add-on output with different names, classes and races.

  python3 dev/make_sample.py OUT_DIR
"""
import sys, pathlib, copy, random
import lupa.lua51 as lupa

ROOT = pathlib.Path(__file__).resolve().parents[1]
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
lua = lupa.LuaRuntime(unpack_returned_tuples=True)
lua.execute((ROOT / "crates/core/tests/fixtures/AltLedger.forever.lua").read_text())
db = lua.globals().AltLedgerDB
base = list(db.characters.values())[0]

def to_py(t):
    if lupa.lua_type(t) == 'table':
        return {k: to_py(v) for k, v in t.items()}
    return t

def to_lua(v, ind=""):
    if isinstance(v, bool): return "true" if v else "false"
    if isinstance(v, (int, float)): return repr(v)
    if isinstance(v, str): return '"' + v.replace('\\', '\\\\').replace('"', '\\"') + '"'
    items = sorted(v.items(), key=lambda kv: (isinstance(kv[0], str), kv[0]))
    inner = "".join(f'{ind}\t[{k if isinstance(k, int) else to_lua(k)}] = {to_lua(x, ind + chr(9))},\n' for k, x in items)
    return "{\n" + inner + ind + "}"

b = to_py(base)
people = [
  ("Faashri Galeforge", 11, "Druid", "DRUID", 95, "Skyborne", 13, 9308, 3),
  ("Norasonia Galeforge", 2, "Paladin", "PALADIN", 3, "Dwarf", 15, 48211, 3),
  ("Pyrela Galeforge", 8, "Mage", "MAGE", 7, "Gnome", 12, 762, 3),
  ("Brunur Galeforge", 4, "Rogue", "ROGUE", 3, "Dwarf", 9, 1520, 2),
  ("Brunsmash Galeforge", 1, "Warrior", "WARRIOR", 7, "Gnome", 11, 522, 2),
  ("Tadincath Galeforge", 3, "Hunter", "HUNTER", 4, "Night Elf", 10, 195, 2),
  ("Vexley Galeforge", 9, "Warlock", "WARLOCK", 7, "Gnome", 1, 0, 3),
]
chars = {}
random.seed(4)
for i, (name, cid, cname, ctok, rid, race, lvl, money, sex) in enumerate(people):
    c = copy.deepcopy(b)
    first = name.split()[0]
    c.update(name=name, firstName=first, secondName="Galeforge", classId=cid)
    c["class"] = cname; c["classToken"] = ctok; c["raceId"] = rid; c["race"] = race
    c["level"] = lvl; c["money"] = money; c["sex"] = sex; c["guid"] = f"Player-1-{i:08X}"
    c["xp"] = int(c["xpMax"] * random.uniform(.1, .9)); c["restXP"] = int(c["xpMax"] * random.uniform(0, 1.4))
    c["lastLogout"] = c["lastLogout"] - i * 7000
    if i: c.pop("bank", None); c.pop("bankScanned", None)
    chars[c["guid"]] = c
db_py = {"schema": 1, "game": "classic", "build": "1.60.1", "characters": chars}
(out / "AltLedger.lua").write_text("\nAltLedgerDB = " + to_lua(db_py) + "\n")
print("wrote", out / "AltLedger.lua")
