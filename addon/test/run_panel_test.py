"""
Opens the in-game panel (Rosterbook_UI) inside Lua 5.1 against a mocked WoW
API and clicks through everything: every tab, every sort, the item search,
switching characters, forgetting one, the minimap button and slash commands.

Widgets are mocked strictly: calling a method WoW's widgets don't have is an
error, so typos fail here instead of in game. It can't show what the panel
looks like; it proves the code runs and prints what each tab would list.

  python3 addon/test/run_panel_test.py [--retail] [--quiet]
"""
import sys
from run_addon_test import new_runtime, load_addon, play_session

IS_RETAIL = "--retail" in sys.argv
QUIET = "--quiet" in sys.argv

WIDGETS = r'''
-- ---------------------------------------------------------------- widgets
local METHODS = {}
for name in ([[
  SetPoint ClearAllPoints SetAllPoints SetSize SetWidth SetHeight SetFrameLevel
  SetToplevel SetMovable EnableMouse EnableMouseWheel RegisterForDrag RegisterForClicks
  StopMovingOrSizing SetBackdrop SetBackdropBorderColor SetHighlightTexture
  LockHighlight UnlockHighlight SetTexture SetTexCoord SetColorTexture SetJustifyH SetWordWrap
  SetTextColor SetStatusBarTexture SetStatusBarColor SetMinMaxValues SetValue
  SetScrollChild SetAutoFocus SetFocus ClearFocus SetOwner AddLine SetItemByID SetHyperlink
  SetOrientation SetValueStep SetObeyStepOnDrag SetThumbTexture SetNormalTexture SetPushedTexture
  SetCheckedTexture CopyFontObject SetTextInsets
]]):gmatch("%S+") do METHODS[name] = function() end end

local registry = {}
local W = {}
W.__index = function(self, k)
  local m = METHODS[k]
  -- Widget methods are capitalised; anything else is the add-on's own field.
  if m == nil and type(k) == "string" and k:match("^%u") then
    error(("%s has no method %s"):format(rawget(self, "_kind"), k), 2)
  end
  return m
end

local function new(kind, name, parent)
  local w = setmetatable({ _kind = kind, _scripts = {}, _events = {}, _shown = true, _text = "",
                           _w = 100, _h = 20, _children = {}, _parent = parent, _scroll = 0 }, W)
  if parent then parent._children[#parent._children + 1] = w end
  if name then _G[name] = w end
  registry[#registry + 1] = w
  return w
end
WidgetRegistry = registry

function METHODS:SetScript(k, fn) self._scripts[k] = fn end
function METHODS:GetScript(k) return self._scripts[k] end
function METHODS:RegisterEvent(e) self._events[e] = true end
function METHODS:UnregisterEvent(e) self._events[e] = nil end
function METHODS:Show()
  local was = self._shown; self._shown = true
  if not was and self._scripts.OnShow then self._scripts.OnShow(self) end
end
function METHODS:Hide() self._shown = false end
function METHODS:IsShown() return self._shown end
function METHODS:IsVisible()
  local w = self
  while w do if not w._shown then return false end w = w._parent end
  return true
end
function METHODS:SetText(t)
  self._text = t or ""
  if self._fs then self._fs._text = self._text end
  if self._kind == "EditBox" and self._scripts.OnTextChanged then self._scripts.OnTextChanged(self, true) end
end
function METHODS:GetText() return self._text end
function METHODS:GetStringWidth() return #(self._text or "") * 6 end
function METHODS:SetSize(w, h) self._w, self._h = w, h end
function METHODS:SetWidth(w) self._w = w end
function METHODS:SetHeight(h) self._h = h end
function METHODS:GetWidth() return self._w end
function METHODS:GetHeight() return self._h end
function METHODS:GetPoint() return "TOPLEFT", UIParent, "TOPLEFT", 120, -80 end
function METHODS:GetCenter() return 500, 400 end
function METHODS:GetEffectiveScale() return 1 end
function METHODS:SetVerticalScroll(v) self._scroll = v end
function METHODS:GetVerticalScroll() return self._scroll end
function METHODS:CreateFontString() return new("FontString", nil, self) end
function METHODS:CreateTexture() return new("Texture", nil, self) end
function METHODS:GetFontString()
  if not self._fs then self._fs = new("FontString", nil, self); self._fs._text = self._text end
  return self._fs
end
function METHODS:SetValue(v)
  local old = self._value; self._value = v
  if old ~= v and self._scripts.OnValueChanged then self._scripts.OnValueChanged(self, v, false) end
end
function METHODS:GetValue() return self._value end
function METHODS:SetChecked(v) self._checked = v and true or false end
function METHODS:GetChecked() return self._checked end
function METHODS:SetScale(v) self._scale = v end
function METHODS:GetScale() return self._scale or 1 end
function METHODS:SetFrameStrata(v) self._strata = v end
function METHODS:SetClampedToScreen(v) self._clamped = v end
function METHODS:SetBackdropColor(r, g, b, a) self._bgAlpha = a end
function METHODS:SetFontObject(f) self._font = f end
function METHODS:SetFont(path, size, flags) self._fontSize = size end
function METHODS:GetFont() return "Fonts\\FRIZQT__.TTF", self._fontSize or 10, "" end
Moves = 0
function METHODS:StartMoving() Moves = Moves + 1 end
function METHODS:Click(button)
  if self._scripts.OnClick then self._scripts.OnClick(self, button or "LeftButton") end
end

local templates = { UIPanelScrollFrameTemplate = true, UIPanelButtonTemplate = true, UIPanelCloseButton = true,
                    InputBoxTemplate = true, BackdropTemplate = true }
function CreateFrame(kind, name, parent, template)
  if template and not templates[template] then error("unknown template " .. template) end
  local f = new(kind, name, parent)
  if kind == "Button" and template == "UIPanelButtonTemplate" then f:GetFontString() end
  return f
end
function FireEvent(e, ...)
  for _, f in ipairs(registry) do
    if f._events[e] and f._scripts.OnEvent then f._scripts.OnEvent(f, e, ...) end
  end
end

UIParent = new("Frame", "UIParent")
function CreateFont(name) return new("Font", name) end
GameFontHighlightSmall = CreateFont("GameFontHighlightSmall"); GameFontHighlightSmall:SetFont("x", 10)
GameFontNormal = CreateFont("GameFontNormal"); GameFontNormal:SetFont("x", 12)
GameFontNormalLarge = CreateFont("GameFontNormalLarge"); GameFontNormalLarge:SetFont("x", 16)
GameFontDisableSmall = CreateFont("GameFontDisableSmall"); GameFontDisableSmall:SetFont("x", 10)
Minimap = new("Frame", "Minimap"); Minimap._w = 140
GameTooltip = new("GameTooltip", "GameTooltip")
UISpecialFrames = {}
tinsert = table.insert
YES, NO = "Yes", "No"
function GetCursorPosition() return 560, 400 end
function IsModifiedClick() return ShiftHeld == true end
LinkedItems = {}
function HandleModifiedItemClick(link) LinkedItems[#LinkedItems + 1] = link end
function GetCoinTextureString(c) return ("%dg %ds %dc"):format(c / 10000, (c / 100) % 100, c % 100) end
function GetItemIcon(id) return 134400 end

RAID_CLASS_COLORS = {
  DRUID = { r = 1, g = 0.49, b = 0.04, colorStr = "ffff7c0a" },
  WARRIOR = { r = 0.78, g = 0.61, b = 0.43, colorStr = "ffc69b6d" },
  PALADIN = { r = 0.96, g = 0.55, b = 0.73, colorStr = "fff48cba" },
}
CLASS_ICON_TCOORDS = { DRUID = { 0.75, 1, 0, 0.25 }, WARRIOR = { 0, 0.25, 0, 0.25 }, PALADIN = { 0, 0.25, 0.5, 0.75 } }
ITEM_QUALITY_COLORS = {}
for q = 0, 7 do ITEM_QUALITY_COLORS[q] = { r = 1, g = 1, b = 1 } end
FACTION_BAR_COLORS = {}
for i = 1, 8 do FACTION_BAR_COLORS[i] = { r = 0, g = 0.6, b = 0.1 }; _G["FACTION_STANDING_LABEL" .. i] = "Standing" .. i end
HEADSLOT, CHESTSLOT, MAINHANDSLOT = "Head", "Chest", "Main Hand"

Popups = {}
function StaticPopup_Show(which, a, b, data) Popups[#Popups + 1] = { which = which, text = a, data = data } end

-- Add-on loading: Rosterbook_UI is load-on-demand. Its saved variables are
-- restored after its file runs, then ADDON_LOADED fires, as in the game.
local loaded = {}
UIAddonPresent = true
SavedUIDB = nil
C_AddOns = {
  GetAddOnInfo = function(name)
    if name == "Rosterbook_UI" and UIAddonPresent then return name, "RosterBook", "", true, "DEMAND_LOADED" end
    if name == "Rosterbook_UI" then return name, nil, nil, false, "MISSING" end
    error("Invalid AddOn name")
  end,
  IsAddOnLoaded = function(name) return loaded[name] == true end,
  LoadAddOn = function(name)
    if not UIAddonPresent then return false, "MISSING" end
    LoadAddOnFile(name)
    RosterbookUIDB = SavedUIDB
    loaded[name] = true
    FireEvent("ADDON_LOADED", name)
    return true
  end,
}
'''

DUMP = r'''
-- Text of every visible row in a scroll list, in order.
local function visibleText(w, out, depth)
  if not w._shown then return end
  if w._kind == "FontString" and w._text ~= "" then out[#out + 1] = w._text end
  for _, c in ipairs(w._children) do visibleText(c, out, depth + 1) end
end
function DumpRows(scroll)
  local lines = {}
  local child = scroll._children[1]
  for _, row in ipairs(child._children) do
    if row._shown then
      local parts = {}
      visibleText(row, parts, 0)
      lines[#lines + 1] = table.concat(parts, "  |  ")
    end
  end
  return table.concat(lines, "\n")
end
function StripCodes(s)
  return (s:gsub("|c%x%x%x%x%x%x%x%x", ""):gsub("|r", ""):gsub("||", "|"))
end
'''

lua = new_runtime(IS_RETAIL)
lua.execute(WIDGETS)
lua.execute(DUMP)
if IS_RETAIL:
    # Pretend another add-on (Titan Panel, ElvUI…) provides LibDataBroker.
    lua.execute('''
      Brokers = {}
      local LDB = { NewDataObject = function(self, name, obj) Brokers[name] = obj; return obj end }
      LibStub = setmetatable({}, { __call = function(_, lib) if lib == "LibDataBroker-1.1" then return LDB end end })
    ''')

from run_addon_test import ROOT
def load_file(name):
    code = (ROOT / name / (name + ".lua")).read_text()
    lua.execute("local f = assert(loadstring(...)); f(select(2, ...))", code, name)
lua.globals().LoadAddOnFile = load_file

load_addon(lua)
play_session(lua, logout=False)
G = lua.globals()

# A second character from an earlier session, with a bank and shared items.
lua.execute('''
  RosterbookDB.characters["Player-4620-00000002"] = {
    guid = "Player-4620-00000002", name = "Brannoc Galeforge", realm = GetRealmName(), level = 9,
    class = "Warrior", classToken = "WARRIOR", race = "Dwarf", faction = "Alliance",
    money = 123456789, xp = 100, xpMax = 4000, restXP = 0, itemLevel = 12.5,
    lastLogout = time() - 3 * 86400, bankScanned = time() - 5 * 86400,
    mail = { count = 3, soonestExpiry = time() + 86400, scanned = time() - 86400 },
    bags = { { id = 0, size = 16, free = 14, items = {
      { slot = 1, id = 2589, name = "Linen Cloth", count = 20, quality = 1 },
      { slot = 2, id = 2589, name = "Linen Cloth", count = 7, quality = 1 } } } },
    bank = { { id = -1, name = "Bank", size = 24, free = 23, items = { { slot = 1, id = 765, name = "Silverleaf", count = 3, quality = 1 } } } },
    equipment = { { slot = 1, id = 1, name = "Dented Helm|pipe", quality = 0 } },
    professions = { { name = "Mining", rank = 40, max = 75, kind = "primary" } },
    reputations = {},
  }
''')

def check(cond, msg):
    if not cond:
        raise SystemExit("FAIL: " + msg)

def show(title, scroll):
    text = G.StripCodes(G.DumpRows(scroll))
    if not QUIET:
        print(f"\n--- {title} ---\n{text}")
    return text

API = G.Rosterbook
check(API.HasPanel(), "panel should be detected as installed")
check(G.RosterbookMinimapButton is not None, "minimap button created at login")
if IS_RETAIL:
    check(G.Brokers["RosterBook"] is not None, "LibDataBroker launcher registered")

# /al opens the panel (loads the add-on on demand)
G.SlashCmdList.ROSTERBOOK("")
panel = G.RosterbookPanel
check(panel is not None and panel.IsShown(panel), "/al opens the panel")

children = list(panel._children.values())
buttons = [c for c in children if c._kind == "Button" and c.fs is not None]
by_label = {b._text: b for b in buttons}
scrolls = [c for c in children if c._kind == "ScrollFrame"]
char_scroll, detail_scroll, grid_scroll = scrolls[0], scrolls[1], scrolls[2]
GRID_TABS = {"Gear", "Bags", "Bank"}

chars = show("Characters", char_scroll)
check("Brannoc Galeforge" in chars and "(you)" in chars, "both characters listed, current marked")

for label in ["Overview", "Bags", "Bank", "Gear", "Reputation", "Currency"]:
    by_label[label].Click(by_label[label])
    scroll = grid_scroll if label in GRID_TABS else detail_scroll
    out = show(label, scroll)
    check(out.strip() != "", f"{label} tab shows rows")

by_label["Currency"].Click(by_label["Currency"])
currency = show("Currency (recheck)", detail_scroll)
check("Merchant's Favor" in currency and "45" in currency, "uncapped currency shown")
check("Honor Points" in currency and "2,450" in currency and "4,000" in currency, "capped currency shows current / max")
check("Darkmoon Prize Ticket" in currency, "currency nested two header levels deep (behind both Seasonal and Darkmoon Faire) is still found")
check("Seasonal" not in currency and "Darkmoon Faire" not in currency, "collapsed currency headers themselves aren't listed as rows")

by_label["Overview"].Click(by_label["Overview"])
prof_name = "Blacksmithing" if IS_RETAIL else "Leatherworking"
check(prof_name in show("Overview (professions folded in)", detail_scroll),
      "profession bars folded into overview")

by_label["Bags"].Click(by_label["Bags"])
check("Hearthstone" in show("Bags (grid)", grid_scroll), "bag items shown as a grid")

# Bank tab on Retail includes the Warband bank
by_label["Bank"].Click(by_label["Bank"])
bank = G.StripCodes(G.DumpRows(grid_scroll))
if IS_RETAIL:
    check("Warband bank" in bank and "Bismuth" in bank, "Warband bank shown")

# Every sort order
sort_btn = next(b for b in buttons if b._text.startswith("Sort:"))
seen = set()
for _ in range(6):
    sort_btn.Click(sort_btn)
    seen.add(sort_btn._text)
check(len(seen) == 5, f"cycles through 5 sorts, saw {sorted(seen)}")
check(G.RosterbookUIDB.sort in ("level", "name", "gold", "ilvl", "played"), "sort saved")

# Item search across characters — a standing search bar, not a tab
edit = next(c for c in children if c._kind == "EditBox")
edit.SetText(edit, "l")
check("two letters" in G.StripCodes(G.DumpRows(detail_scroll)), "asks for 2+ letters")
edit.SetText(edit, "linen")
found = show("Find: linen", detail_scroll)
check("Linen Cloth" in found and "Brannoc Galeforge" in found, "finds linen on the alt")
check("bags 27" in found, "stacks on one character are added together")
edit.SetText(edit, "|silver")
edit.SetText(edit, "silver")
found = show("Find: silver", detail_scroll)
check("Silverleaf" in found and "bank" in found, "finds bank items")
edit.SetText(edit, "zzzz|q")
check("No items match" in G.StripCodes(G.DumpRows(detail_scroll)), "no-match message")

# Shift-click links an item into chat
edit.SetText(edit, "linen")
G.ShiftHeld = True
item_row = detail_scroll._children[1]._children[1]
item_row._scripts.OnClick(item_row, "LeftButton")
G.ShiftHeld = False
item_row._scripts.OnEnter(item_row)
edit.SetText(edit, "")

# Switch to the alt, forget it
rows = [r for r in char_scroll._children[1]._children.values() if r._shown]
alt_row = next(r for r in rows if r.spec and "Brannoc" in r.spec.text)
alt_row._scripts.OnClick(alt_row, "LeftButton")
by_label["Overview"].Click(by_label["Overview"])
ov = show("Overview (alt)", detail_scroll)
check("Forget this character" in ov, "alt can be forgotten")
check("first expires in" in ov, "mail expiry shown")
forget = next(r for r in detail_scroll._children[1]._children.values() if r._shown and r.spec and r.spec.onClick)
forget._scripts.OnClick(forget, "LeftButton")
popup = G.Popups[len(G.Popups)]
check(popup.which == "ROSTERBOOK_FORGET" and popup.data == "Player-4620-00000002", "confirmation popup")
G.StaticPopupDialogs.ROSTERBOOK_FORGET.OnAccept(None, popup.data)
check(G.RosterbookDB.characters["Player-4620-00000002"] is None, "alt forgotten")
check(not API.ForgetCharacter(API.GetCurrentGUID()), "can't forget the character you're on")
check("Brannoc" not in G.StripCodes(G.DumpRows(char_scroll)), "list updated")

# Live updates: gold changes while the panel is open
G.GetMoney = lambda: 999999
G.FireEvent("PLAYER_MONEY")
check("99g 99s 99c" in G.StripCodes(G.DumpRows(char_scroll)) or "99g" in G.StripCodes(G.DumpRows(char_scroll)), "refreshes on change")

# Minimap button drag + hide/show, slash commands
mm = G.RosterbookMinimapButton
mm._scripts.OnDragStart(mm)
mm._scripts.OnUpdate(mm)
mm._scripts.OnDragStop(mm)
check(abs(G.RosterbookSettings.minimap.angle - 0) < 1e-6, "angle saved from cursor position")
mm._scripts.OnEnter(mm)
G.SlashCmdList.ROSTERBOOK("minimap")
check(not mm.IsShown(mm) and G.RosterbookSettings.minimap.hide, "minimap button hidden")
G.SlashCmdList.ROSTERBOOK("minimap")
check(mm.IsShown(mm), "minimap button shown again")
for cmd in ["status", "scan", "help", " SCAN "]:
    G.SlashCmdList.ROSTERBOOK(cmd)
G.SlashCmdList.ROSTERBOOK("")
check(not panel.IsShown(panel), "/al toggles it closed")
G.Rosterbook_OnAddonCompartmentClick()
check(panel.IsShown(panel), "add-on compartment opens it")

# ---- Settings
settings_btn = next(c for c in children if c._kind == "Button" and c._scripts.OnClick and c.fs is None and c._w == 18)
by_label["Gear"].Click(by_label["Gear"])
settings_btn.Click(settings_btn)
check(G.RosterbookUIDB.tab == "settings", "gear opens settings")
view = next(c for c in children if c._kind == "Frame" and c._shown and any(
    k._kind == "CheckButton" for k in c._children.values()))
checks = {k.label._text: k for k in view._children.values() if k._kind == "CheckButton"}
sliders = {}
for holder in view._children.values():
    if holder._kind == "Frame" and holder.slider:
        sliders[G.StripCodes(holder.label._text).split(":")[0]] = holder.slider
check(set(sliders) == {"Background opacity", "Text size", "Window scale"}, f"three sliders, got {set(sliders)}")
check(set(checks) == {"Lock window position", "Keep window on screen", "Close with Escape",
                      "Only this realm's characters", "Show minimap button"}, f"checkboxes {set(checks)}")
check(checks["Close with Escape"]._checked and checks["Keep window on screen"]._checked, "defaults ticked")

sliders["Background opacity"].SetValue(sliders["Background opacity"], 0.4)
check(abs(G.RosterbookUIDB.settings.opacity - 0.4) < 1e-9 and abs(panel._bgAlpha - 0.4) < 1e-9, "opacity applied")
sliders["Background opacity"]._scripts.OnMouseWheel(sliders["Background opacity"], -1)
check(abs(G.RosterbookUIDB.settings.opacity - 0.35) < 1e-9, "mouse wheel steps the slider")

sliders["Text size"].SetValue(sliders["Text size"], 14)
check(G.RosterbookFontRow._fontSize == 14 and G.RosterbookFontHeader._fontSize == 16, "text size applied to fonts")
check(char_scroll._children[1]._children[1]._h == 54, "character rows grow with the text")

sliders["Window scale"].SetValue(sliders["Window scale"], 1.25)
check(panel._scale != 1.25, "scale waits for mouse release")
sliders["Window scale"]._scripts.OnMouseUp(sliders["Window scale"])
check(panel._scale == 1.25, "scale applied on release")

checks["Lock window position"].SetChecked(checks["Lock window position"], True)
checks["Lock window position"].Click(checks["Lock window position"])
before = G.Moves
panel._scripts.OnDragStart(panel)
check(G.Moves == before and G.RosterbookUIDB.settings.locked, "locked window doesn't move")

esc = checks["Close with Escape"]; esc.SetChecked(esc, False); esc.Click(esc)
check("RosterbookPanel" not in list(G.UISpecialFrames.values()), "Escape no longer closes it")
esc.SetChecked(esc, True); esc.Click(esc)
check(list(G.UISpecialFrames.values()).count("RosterbookPanel") == 1, "Escape closes it again, listed once")

realm = checks["Only this realm's characters"]
G.RosterbookDB.characters["Player-1-00000009"] = lua.table_from({"guid": "Player-1-00000009", "name": "Farshore Alt",
    "realm": "Somewhere Else", "level": 5, "classToken": "WARRIOR", "money": 1})
realm.SetChecked(realm, True); realm.Click(realm)
check("Farshore" not in G.StripCodes(G.DumpRows(char_scroll)), "other realm hidden")
realm.SetChecked(realm, False); realm.Click(realm)
check("Farshore" in G.StripCodes(G.DumpRows(char_scroll)), "other realm shown")

mmcb = checks["Show minimap button"]; mmcb.SetChecked(mmcb, False); mmcb.Click(mmcb)
check(not G.RosterbookMinimapButton.IsShown(G.RosterbookMinimapButton), "minimap button hidden from settings")

layer = next(b for b in view._children.values() if b._kind == "Button" and b._text == "Above most windows")
layer.Click(layer)
check(G.RosterbookUIDB.settings.strata == "DIALOG" and panel._strata == "DIALOG", "layer cycles")
check(layer._text == "On top of everything", "layer label follows")

restore = next(b for b in view._children.values() if b._kind == "Button" and b._text == "Restore defaults")
restore.Click(restore)
st = G.RosterbookUIDB.settings
check(st.opacity == 0.95 and st.scale == 1 and st.fontSize == 10 and not st.locked and st.strata == "HIGH", "defaults restored")
check(G.RosterbookMinimapButton.IsShown(G.RosterbookMinimapButton) and panel._scale == 1, "defaults applied")
reset = next(b for b in view._children.values() if b._kind == "Button" and b._text == "Reset position")
reset.Click(reset)
check(G.RosterbookUIDB.point is None, "position reset")

settings_btn.Click(settings_btn)
check(G.RosterbookUIDB.tab == "gear", "gear button returns to the last tab")
if not QUIET:
    print("\n--- Settings ---\nsliders:", sorted(sliders), "\nchecks:", sorted(checks))

# Export-only install (panel not installed): no launcher, /al prints status.
bare = new_runtime(IS_RETAIL)
bare.execute(WIDGETS)
bare.execute("UIAddonPresent = false")
load_addon(bare)
play_session(bare, logout=False)
B = bare.globals()
check(not B.Rosterbook.HasPanel(), "panel reported missing")
check(B.RosterbookMinimapButton is None, "no minimap button without the panel")
B.SlashCmdList.ROSTERBOOK("")
B.Rosterbook.Toggle()
check(B.RosterbookPanel is None, "nothing opens without the panel")

print(f"ok: in-game panel ({'Retail' if IS_RETAIL else 'Forever'})")
