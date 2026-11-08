"""
Runs Rosterbook.lua inside Lua 5.1 (the version WoW uses) against a mocked WoW
API, fires the events a real login/bank visit/logout would, then writes the
saved variable the way WoW does. Used to check the add-on and the companion
app's reader together without launching the game.

  python3 addon/test/run_addon_test.py [--retail] [output.lua]
"""
import sys, pathlib
import lupa.lua51 as lupa

ROOT = pathlib.Path(__file__).resolve().parents[1]
ARGS = [a for a in sys.argv[1:] if not a.startswith("--")]
IS_RETAIL = "--retail" in sys.argv
OUT = pathlib.Path(ARGS[0] if ARGS else "/tmp/Rosterbook.lua")

MOCK = r'''
local t = 1792593450
function time() return t end
function GetTime() return 1000 end
function date(f, x) return "Oct 21 10:00" end
print = function(...) end
C_Timer = { After = function(_, fn) fn() end }
Enum = { BagIndex = { Backpack = 0, ReagentBag = 5, CharacterBankTab_1 = 6, CharacterBankTab_6 = 11 } }
NUM_BAG_SLOTS = 4
TRADE_SKILLS, SECONDARY_SKILLS = "Professions", "Secondary Skills"

local frames = {}
function CreateFrame()
  local f = { events = {} }
  function f:RegisterEvent(e) self.events[e] = true end
  function f:SetScript(_, fn) self.onEvent = fn end
  frames[#frames + 1] = f
  return f
end
function FireEvent(e, ...) for _, f in ipairs(frames) do if f.events[e] then f.onEvent(f, e, ...) end end end

function UnitGUID() return "Player-4620-009148B3" end
function UnitName() return "Faashri" end
C_PlayerInfo = { GetSecondaryName = function() return "Galeforge" end }
function GetRealmName() return "Forever" end
function GetBuildInfo() return "1.60.1", "70450", "Oct 1 2026", 16001 end
function UnitLevel() return 13 end
function UnitClass() return "Druid", "DRUID", 11 end
function UnitRace() return "Skyborne", "Skyborne", 95 end
function UnitSex() return 3 end
function UnitFactionGroup() return "Alliance" end
function UnitHealthMax() return 348 end
function UnitPowerType() return 0, "MANA" end
function UnitPowerMax() return 475 end
function GetMoney() return 9308 end
function UnitXP() return 5321 end
function UnitXPMax() return 11400 end
function GetXPExhaustion() return 584 end
function IsResting() return true end
function GetRealZoneText() return "Zephras Isle" end
function GetSubZoneText() return "Valanaar" end
function GetBindLocation() return "Valanaar" end

-- bags: backpack (20) with two stacks, one 6-slot pouch, profession bag
local bags = {
  [0] = { size = 20, items = { [1] = { 6948, 1, 1, "Hearthstone" }, [2] = { 2589, 15, 1, "Linen Cloth" } } },
  [1] = { size = 6, link = "|cnIQ1:|Hitem:805::|h[Small Red Pouch]|h|r", items = { [3] = { 774, 1, 2, "Malachite" } } },
  [5] = { size = 4, link = "|cnIQ1:|Hitem:277114::|h[Apprentice's Skinning Satchel]|h|r", items = {} },
  [6] = { size = 48, link = "|cnIQ1:|Hitem:242709::|h[Character Bank Tab Bag (DNT)]|h|r", bank = true,
          items = { [1] = { 765, 7, 1, "Silverleaf" } } },
}
local bankOpen = false
local function bag(id) local b = bags[id]; if b and (not b.bank or bankOpen) then return b end end
local function link(id, name, q) return ("|cnIQ%d:|Hitem:%d::::|h[%s]|h|r"):format(q, id, name) end
C_Container = {
  GetContainerNumSlots = function(id) local b = bag(id); return b and b.size or 0 end,
  GetContainerNumFreeSlots = function(id) local b = bag(id); if not b then return 0 end
    local n = 0; for _ in pairs(b.items) do n = n + 1 end; return b.size - n end,
  GetContainerItemInfo = function(id, slot) local b = bag(id); local it = b and b.items[slot]
    if it then return { itemID = it[1], stackCount = it[2], quality = it[3], iconFileID = 133000 + it[1], hyperlink = link(it[1], it[4], it[3]) } end end,
  GetContainerItemLink = function() return nil end,
  ContainerIDToInventoryID = function(id) return 100 + id end,
}
local equipped = { [5] = { 257344, "Skywatcher's Vestments", 2 }, [16] = { 257343, "Balanced Quarterstaff", 2 } }
function GetInventoryItemLink(_, slot)
  if slot >= 100 then local b = bags[slot - 100]; return b and b.link end
  local e = equipped[slot]; return e and link(e[1], e[2], e[3])
end
function GetInventoryItemID(_, slot)
  if slot >= 100 then local l = GetInventoryItemLink(nil, slot); return l and tonumber(l:match("item:(%d+)")) end
  local e = equipped[slot]; return e and e[1]
end
function GetInventoryItemQuality(_, slot) local e = equipped[slot]; return e and e[3] end
function GetInventoryItemTexture(_, slot) return 135000 + slot end
function GetItemInfo(l) return "x", l, 2, 5 end
-- itemID -> classID, subClassID, equipLoc (Enum.ItemClass values)
local cats = {
  [6948] = { 15, 0 }, [2589] = { 7, 5 }, [774] = { 7, 4 }, [765] = { 7, 9 }, [210930] = { 7, 7 },
  [257344] = { 4, 2, "INVTYPE_CHEST" }, [257343] = { 2, 10, "INVTYPE_2HWEAPON" },
}
function GetItemInfoInstant(id)
  local c = cats[id]
  if not c then return id, "", "", "", 0, 15, 0 end
  return id, "", "", c[3] or "", 0, c[1], c[2]
end

local skills = {
  { "Professions", true, false }, { "Leatherworking", false, false, 38, 0, 0, 75 }, { "Skinning", false, false, 2, 0, 0, 75 },
  { "Secondary Skills", true, true }, { "Cooking", false, false, 1, 0, 0, 75 },
  { "Weapon Skills", true, true }, { "Staves", false, false, 65, 0, 0, 65 },
}
local skillsVisible
local function visibleSkills()
  local out, show = {}, true
  for _, s in ipairs(skills) do
    if s[2] then out[#out + 1] = s; show = s[3] elseif show then out[#out + 1] = s end
  end
  return out
end
function GetNumSkillLines() return #visibleSkills() end
function GetSkillLineInfo(i) return unpack(visibleSkills()[i]) end
function ExpandSkillHeader(i) visibleSkills()[i][3] = true end
function CollapseSkillHeader(i) visibleSkills()[i][3] = false end

local factions = {
  { "Alliance", true, false, 3000, 9000, 3300, 469, true, 5 },
  { "Stormwind", false, false, 3000, 9000, 3100, 72, false, 5 },
  { "Ironforge", false, false, 0, 3000, 2000, 47, false, 4 },
  { "Skyborne", true, true, 0, 0, 0, 0, false, 0 },
  { "High Order", false, false, 3000, 9000, 5960, 2779, false, 5 },
}
local function visibleFactions()
  local out, show = {}, true
  for _, f in ipairs(factions) do
    if f[2] then out[#out + 1] = f; show = not f[3] elseif show then out[#out + 1] = f end
  end
  return out
end
function GetNumFactions() return #visibleFactions() end
function GetFactionInfo(i)
  local f = visibleFactions()[i]
  -- name, description, standingID, barMin, barMax, barValue, atWarWith, canToggleAtWar, isHeader, isCollapsed, hasRep, isWatched, isChild, factionID
  return f[1], "", f[9], f[4], f[5], f[6], false, false, f[2], f[3], f[8], false, false, f[7]
end
function ExpandFactionHeader(i) visibleFactions()[i][3] = false end
function CollapseFactionHeader(i) visibleFactions()[i][3] = true end

function GetInboxNumItems() return 2 end
function GetInboxHeaderInfo(i) return nil, nil, "Pyrela", "Ore", 0, 0, i == 1 and 29.5 or 2.25 end
function RequestTimePlayed() FireEvent("TIME_PLAYED_MSG", 35556, 2147) end
SlashCmdList = {}

function SetBankOpen(v) bankOpen = v end
'''

# Applied on top of MOCK to look like Retail 12.x: no skill list or
# GetFactionInfo, professions via GetProfessions, renown/friendship factions,
# named bank tabs and a Warband bank.
RETAIL = r'''
WOW_PROJECT_MAINLINE, WOW_PROJECT_ID = 1, 1
function GetBuildInfo() return "12.1.5", "70448", "Oct 1 2026", 120105 end
GetNumSkillLines, GetSkillLineInfo, GetFactionInfo, GetNumFactions, GetItemInfo = nil, nil, nil, nil, nil
ExpandFactionHeader, CollapseFactionHeader = nil, nil
function GetAverageItemLevel() return 612.4, 610.8 end
C_Item = { GetItemInfo = function(l) return "x", l, 4, 610 end }
function UnitLevel() return 80 end
function IsPlayerAtEffectiveMaxLevel() return true end
function UnitClass() return "Paladin", "PALADIN", 2 end
function UnitRace() return "Dwarf", "Dwarf", 3 end
function UnitName() return "Norasonia" end
C_PlayerInfo = {}

local profs = { { "Blacksmithing", 85, 100, "Khaz Algar Blacksmithing" }, { "Mining", 100, 100, "Khaz Algar Mining" }, nil, nil, { "Cooking", 12, 100, "Khaz Algar Cooking" } }
function GetProfessions() return 1, 2, nil, nil, 5 end
function GetProfessionInfo(i) local p = profs[i]; return p[1], 0, p[2], p[3], 0, 0, 0, 0, 0, 0, p[4] end

local rfac = {
  { name = "Khaz Algar", isHeader = true, isCollapsed = true, factionID = 0 },
  { name = "Council of Dornogal", factionID = 2590, reaction = 8, currentStanding = 0, currentReactionThreshold = 0, nextReactionThreshold = 0 },
  { name = "Brann Bronzebeard", factionID = 2640, reaction = 5, currentStanding = 0, currentReactionThreshold = 0, nextReactionThreshold = 0 },
  { name = "Classic", isHeader = true, isCollapsed = false, factionID = 0 },
  { name = "Ironforge", factionID = 47, reaction = 8, currentStanding = 42999, currentReactionThreshold = 42000, nextReactionThreshold = 42999 },
}
local function visR()
  local out, show = {}, true
  for _, f in ipairs(rfac) do
    if f.isHeader then out[#out + 1] = f; show = not f.isCollapsed elseif show then out[#out + 1] = f end
  end
  return out
end
C_Reputation = {
  GetNumFactions = function() return #visR() end,
  GetFactionDataByIndex = function(i) return visR()[i] end,
  ExpandFactionHeader = function(i) visR()[i].isCollapsed = false end,
  CollapseFactionHeader = function(i) visR()[i].isCollapsed = true end,
  IsMajorFaction = function(id) return id == 2590 end,
}
C_MajorFactions = { GetMajorFactionData = function() return { renownLevel = 22, renownReputationEarned = 1250, renownLevelThreshold = 2500 } end }
C_GossipInfo = { GetFriendshipReputation = function(id)
  if id == 2640 then return { friendshipFactionID = 2640, reaction = "Level 45", standing = 3100, reactionThreshold = 3000, nextThreshold = 4000 } end
  return { friendshipFactionID = 0 } end }

Enum.BankType = { Character = 0, Account = 2 }
Enum.BagIndex.AccountBankTab_1, Enum.BagIndex.AccountBankTab_5 = 12, 16
C_Bank = {
  FetchPurchasedBankTabData = function(t)
    if t == 0 then return { { ID = 6, name = "Mats" } } end
    return { { ID = 12, name = "Shared Ore" } }
  end,
  FetchDepositedMoney = function() return 1234567 end,
}
'''

SERIALIZE = r'''
local function key(k)
  if type(k) == "number" then return "[" .. k .. "]" end
  return ("[%q]"):format(k)
end
local function val(v, indent)
  local tv = type(v)
  if tv == "string" then return ("%q"):format(v) end
  if tv == "number" or tv == "boolean" then return tostring(v) end
  local parts = { "{\n" }
  local keys = {}
  for k in pairs(v) do keys[#keys + 1] = k end
  table.sort(keys, function(a, b)
    if type(a) == type(b) then return a < b end
    return type(a) == "number"
  end)
  for _, k in ipairs(keys) do
    parts[#parts + 1] = indent .. "\t" .. key(k) .. " = " .. val(v[k], indent .. "\t") .. ",\n"
  end
  parts[#parts + 1] = indent .. "}"
  return table.concat(parts)
end
return "\nRosterbookDB = " .. val(RosterbookDB, "") .. "\n"
'''

def new_runtime(retail):
    """A Lua 5.1 runtime with the mocked WoW API (Forever, or Retail)."""
    lua = lupa.LuaRuntime(unpack_returned_tuples=True)
    lua.execute(MOCK)
    if retail:
        lua.execute(RETAIL)
        # A Warband bank tab with one stack, visible only while the bank is open.
        lua.execute('''
          local orig = C_Container.GetContainerNumSlots
          local origInfo = C_Container.GetContainerItemInfo
          local origFree = C_Container.GetContainerNumFreeSlots
          local open = false
          local oldSet = SetBankOpen
          SetBankOpen = function(v) open = v; oldSet(v) end
          C_Container.GetContainerNumSlots = function(id) if id == 12 then return open and 98 or 0 end return orig(id) end
          C_Container.GetContainerNumFreeSlots = function(id) if id == 12 then return 97 end return origFree(id) end
          C_Container.GetContainerItemInfo = function(id, slot)
            if id == 12 then if open and slot == 1 then return { itemID = 210930, stackCount = 200, quality = 1, iconFileID = 1, hyperlink = "|cnIQ1:|Hitem:210930::|h[Bismuth]|h|r" } end return nil end
            return origInfo(id, slot)
          end
        ''')
    return lua


def load_addon(lua, folder="Rosterbook"):
    code = (ROOT / folder / (folder + ".lua")).read_text()
    lua.execute("local f = assert(loadstring(...)); f(select(2, ...))", code, folder)


def play_session(lua, logout=True):
    """Fires what a real login, bank visit, mailbox check and logout would."""
    fire = lua.globals().FireEvent
    for e in ["PLAYER_LOGIN", "PLAYER_ENTERING_WORLD", "BAG_UPDATE_DELAYED"]:
        fire(e)
    lua.globals().SetBankOpen(True)
    fire("BANKFRAME_OPENED")
    fire("BANKFRAME_CLOSED")
    lua.globals().SetBankOpen(False)
    fire("BAG_UPDATE_DELAYED")        # bank closed: must not wipe saved bank
    fire("MAIL_INBOX_UPDATE")
    if logout:
        fire("PLAYER_LOGOUT")


if __name__ == "__main__":
    lua = new_runtime(IS_RETAIL)
    load_addon(lua)
    play_session(lua)
    OUT.write_text(lua.execute(SERIALIZE))
    print(f"ok: wrote {OUT}")
