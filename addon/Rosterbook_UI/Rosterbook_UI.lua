--[[
  RosterBook — in-game panel

  An optional window over the data the RosterBook add-on records. It's
  load-on-demand: nothing here runs, or uses memory, until you open it with
  /rb, the minimap button or the add-on compartment.

  It only ever reads RosterbookDB (through the RosterBook API), with one
  exception: "Forget character" removes a character the player chose.
  Its own settings (window position, sort, last tab) live in RosterbookUIDB.

  Free and open source (MIT), like the add-on it belongs to.
]]

local API = Rosterbook
if not API then return end

local Panel = {}
API.Panel = Panel

-- Shared media (fonts/borders) other add-ons register, e.g. ElvUI, Ellesmere
-- UI or a standalone LibSharedMedia-3.0 media pack. Optional: nil if nothing
-- in the load order provides it, and every call below falls back cleanly.
local LSM = LibStub and LibStub:GetLibrary("LibSharedMedia-3.0", true)

-- The EllesmereUI skin table, set once build() registers with it. Nil until
-- then (or always, if EUI isn't installed or skinning is turned off), and
-- every call site below checks it, falling back to our own flat look.
local EUI_Skin

-- ---------------------------------------------------------------------------
-- Settings
-- ---------------------------------------------------------------------------
local cfg

-- Text size the game uses for small text (varies by client and locale).
local BASE_FONT_SIZE = math.floor(select(2, GameFontHighlightSmall:GetFont()) + 0.5)

local DEFAULTS = {
  opacity = 0.95,        -- background only; text and borders stay solid
  scale = 1.0,
  fontSize = BASE_FONT_SIZE,
  locked = false,
  clamp = true,          -- keep the window on screen
  escape = true,         -- Escape closes the window
  strata = "HIGH",
  thisRealmOnly = false,
  borderStyle = "None",  -- LibSharedMedia border name, or "None" for the flat built-in look
  fontStyle = "Default", -- LibSharedMedia font name, or "Default" for the game's own font
  useEUISkin = true,     -- match EllesmereUI's look when that add-on is installed
}

local function loadSettings()
  RosterbookUIDB = RosterbookUIDB or {}
  cfg = RosterbookUIDB
  cfg.sort = cfg.sort or "level"
  cfg.tab = cfg.tab or "overview"
  -- "settings" is a popover, not a tab; "find" was folded into a standalone
  -- search bar in a later version. "professions" used to be folded into
  -- Overview too, but it's back as its own tab (with recipes).
  if cfg.tab == "settings" or cfg.tab == "find" then cfg.tab = "overview" end
  cfg.settings = cfg.settings or {}
  for k, v in pairs(DEFAULTS) do
    if cfg.settings[k] == nil then cfg.settings[k] = v end
  end
end

-- ---------------------------------------------------------------------------
-- Fonts: the panel's own font objects, so one setting resizes every line.
-- ---------------------------------------------------------------------------
local Fonts = {
  row = CreateFont("RosterbookFontRow"),
  header = CreateFont("RosterbookFontHeader"),
  title = CreateFont("RosterbookFontTitle"),
}
Fonts.row:CopyFontObject(GameFontHighlightSmall)
Fonts.header:CopyFontObject(GameFontNormal)
Fonts.title:CopyFontObject(GameFontNormalLarge)

-- Right-edge x offsets (from the row's own right edge) for the character
-- list's RXP and Gold columns, shared by the rows and their header labels
-- so the two line up regardless of either column's text width.
local GOLD_COL_X, RXP_COL_X = -4, -56

-- Row geometry follows the text size (these are the sizes at 10pt).
local Metrics = {}
local function applyFonts(size, fontPath)
  local function set(font, from, delta)
    local path, _, flags = from:GetFont()
    font:SetFont(fontPath or path, size + delta, flags or "")
  end
  set(Fonts.row, GameFontHighlightSmall, 0)
  set(Fonts.header, GameFontNormal, 2)
  -- A serif display face for the big character-name heading, closer to the
  -- desktop app's branding than the game's default UI font — unless a
  -- shared-media font was picked, which then wins everywhere for consistency.
  Fonts.title:SetFont(fontPath or "Fonts\\MORPHEUS.ttf", size + 10, "")
  local d = size - 10
  Metrics.header, Metrics.text, Metrics.item, Metrics.bar, Metrics.space = 24 + d, 18 + d, 20 + d, 22 + d, 8
  Metrics.icon, Metrics.barHeight, Metrics.barLeft = 16 + d, 14 + d, 150 + d * 8
  Metrics.character = 2 * (18 + d) + 10
end
applyFonts(BASE_FONT_SIZE)

-- ---------------------------------------------------------------------------
-- Formatting helpers
-- ---------------------------------------------------------------------------
local GOLD = "|cffd9a441"
local GREY = "|cff9d9d9d"
local WHITE = "|cffffffff"
local ACCENT_HEX = "|cff9d8cff"
local QUESTION_MARK = "Interface\\Icons\\INV_Misc_QuestionMark"

-- Flat colour theme, to match the desktop app's look.
local THEME = {
  bg = { 0.067, 0.078, 0.102 },      -- window background
  border = { 0.16, 0.18, 0.22 },     -- window border / divider
  accent = { 0.62, 0.55, 0.95 },     -- lavender accent (selection, active tab)
}

local GetItemIconCompat = (C_Item and C_Item.GetItemIconByID) or GetItemIcon
local GetItemInfoCompat = (C_Item and C_Item.GetItemInfo) or GetItemInfo

local function money(copper)
  copper = math.floor(copper or 0)
  if GetCoinTextureString then return GetCoinTextureString(copper) end
  return ("%dg %ds %dc"):format(copper / 10000, (copper / 100) % 100, copper % 100)
end

-- Short money for the narrow character list: gold when there's at least
-- one, else silver, else copper — so a character under 1g doesn't just
-- read "0g" (the full money() breakdown is still shown once selected).
local function commas(n)
  return tostring(math.floor(n or 0)):reverse():gsub("(%d%d%d)", "%1,"):reverse():gsub("^,", "")
end
local function goldShort(copper)
  copper = math.floor(copper or 0)
  local gold = math.floor(copper / 10000)
  if gold > 0 then return commas(gold) .. "|cffffd700g|r" end
  local silver = math.floor(copper / 100)
  if silver > 0 then return silver .. "|cffc7c7cfs|r" end
  return copper .. "|cffeda55fc|r"
end

local function duration(seconds)
  seconds = math.floor(seconds or 0)
  local d, h, m = math.floor(seconds / 86400), math.floor(seconds / 3600) % 24, math.floor(seconds / 60) % 60
  if d > 0 then return ("%dd %dh"):format(d, h) end
  if h > 0 then return ("%dh %dm"):format(h, m) end
  return ("%dm"):format(m)
end

local function ago(t)
  if not t then return "never" end
  local s = time() - t
  if s < 120 then return "just now" end
  if s < 3600 then return ("%d min ago"):format(s / 60) end
  if s < 86400 then return ("%d h ago"):format(s / 3600) end
  return ("%d days ago"):format(s / 86400)
end

local function classColor(token)
  local c = token and RAID_CLASS_COLORS and RAID_CLASS_COLORS[token]
  if not c then return 1, 1, 1, "ffffffff" end
  local hex = c.colorStr or ("ff%02x%02x%02x"):format(c.r * 255, c.g * 255, c.b * 255)
  return c.r, c.g, c.b, hex
end

local function qualityColor(q)
  local c = q and ITEM_QUALITY_COLORS and ITEM_QUALITY_COLORS[q]
  if c then return c.r, c.g, c.b end
  return 1, 1, 1
end

local function itemIcon(item)
  if item.icon then return item.icon end
  return (item.id and GetItemIconCompat and GetItemIconCompat(item.id)) or QUESTION_MARK
end

local SLOT_KEYS = {
  "HEADSLOT", "NECKSLOT", "SHOULDERSLOT", "SHIRTSLOT", "CHESTSLOT", "WAISTSLOT", "LEGSSLOT", "FEETSLOT",
  "WRISTSLOT", "HANDSSLOT", "FINGER0SLOT", "FINGER1SLOT", "TRINKET0SLOT", "TRINKET1SLOT", "BACKSLOT",
  "MAINHANDSLOT", "SECONDARYHANDSLOT", "RANGEDSLOT", "TABARDSLOT",
}
local SLOT_FALLBACK = {
  "Head", "Neck", "Shoulder", "Shirt", "Chest", "Waist", "Legs", "Feet", "Wrist", "Hands", "Finger", "Finger",
  "Trinket", "Trinket", "Back", "Main Hand", "Off Hand", "Ranged", "Tabard",
}
local function slotName(slot) return _G[SLOT_KEYS[slot] or ""] or SLOT_FALLBACK[slot] or ("Slot " .. slot) end

local function lastPlayed(c)
  if c.guid == API.GetCurrentGUID() then return time() end
  return c.lastLogout or c.lastUpdate or 0
end

local function hasRestBonus(c)
  if c.maxLevel or c.xpDisabled or not c.xpMax or c.xpMax == 0 then return false end
  return (c.restXP or 0) > 0
end

local function hasMail(c) return c.mail and (c.mail.count or 0) > 0 end

-- Rested XP as a percentage of the XP needed for the next level (what the
-- in-game rested bar itself measures against) — "—" when there's no rest
-- bonus (including a max-level or XP-disabled character).
local function restPctText(c)
  if not hasRestBonus(c) then return "—" end
  return math.floor(c.restXP / c.xpMax * 100 + 0.5) .. "%"
end

-- Used/total bag slots across every carried bag (for the Bags tab label).
local function bagUsedTotal(c)
  local used, total = 0, 0
  for _, bag in ipairs(c.bags or {}) do
    total = total + (bag.size or 0)
    used = used + (bag.size or 0) - (bag.free or 0)
  end
  return used, total
end

-- ---------------------------------------------------------------------------
-- Characters
-- ---------------------------------------------------------------------------
local SORTS = {
  { key = "level", label = "Level", value = function(c) return (c.level or 0) * 1e6 + (c.xp or 0) end, desc = true },
  { key = "name", label = "Name", value = function(c) return (c.name or ""):lower() end },
  { key = "gold", label = "Gold", value = function(c) return c.money or 0 end, desc = true },
  { key = "ilvl", label = "Item level", value = function(c) return c.itemLevel or 0 end, desc = true },
  { key = "played", label = "Last played", value = lastPlayed, desc = true },
}
local function sortSpec()
  for i, s in ipairs(SORTS) do if s.key == cfg.sort then return s, i end end
  return SORTS[1], 1
end

local function characters()
  local list = {}
  local db = API.GetDB()
  local realm = cfg.settings.thisRealmOnly and GetRealmName()
  for guid, c in pairs(db and db.characters or {}) do
    if c.name and (not realm or c.realm == realm or guid == API.GetCurrentGUID()) then
      c.guid = c.guid or guid
      list[#list + 1] = c
    end
  end
  local s = sortSpec()
  table.sort(list, function(a, b)
    local va, vb = s.value(a), s.value(b)
    if va == vb then return (a.name or "") < (b.name or "") end
    if s.desc then return va > vb end
    return va < vb
  end)
  return list
end

-- ---------------------------------------------------------------------------
-- Widgets
-- ---------------------------------------------------------------------------
local function tryCreate(kind, name, parent, template)
  if template then
    local ok, f = pcall(CreateFrame, kind, name, parent, template)
    if ok and f then return f end
  end
  return CreateFrame(kind, name, parent)
end

local function setBackdrop(f, alpha)
  if not f.SetBackdrop then return end
  f:SetBackdrop({
    bgFile = "Interface\\Buttons\\WHITE8x8",
    edgeFile = "Interface\\Buttons\\WHITE8x8",
    edgeSize = 1,
  })
  f:SetBackdropColor(THEME.bg[1], THEME.bg[2], THEME.bg[3], alpha or 0.96)
  f:SetBackdropBorderColor(THEME.border[1], THEME.border[2], THEME.border[3], 1)
end

-- A flat, borderless-background button — built by hand instead of from
-- UIPanelButtonTemplate, so a global button-reskinning add-on (common ones
-- recolour every Blizzard-template button, e.g. red) has no stock texture of
-- ours to recolour.
local function button(parent, text, width, onClick)
  local b = tryCreate("Button", nil, parent, "BackdropTemplate")
  b:SetSize(width, 22)
  if b.SetBackdrop then
    b:SetBackdrop({ bgFile = "Interface\\Buttons\\WHITE8x8", edgeFile = "Interface\\Buttons\\WHITE8x8", edgeSize = 1 })
    b:SetBackdropColor(1, 1, 1, 0.04)
    b:SetBackdropBorderColor(THEME.border[1], THEME.border[2], THEME.border[3], 1)
  end
  b.fs = b:CreateFontString(nil, "OVERLAY")
  b.fs:SetFontObject(Fonts.row)
  b.fs:SetPoint("CENTER")
  b.hl = b:CreateTexture(nil, "HIGHLIGHT")
  b.hl:SetAllPoints()
  b.hl:SetColorTexture(1, 1, 1, 0.08)
  b.SetText = function(self, t) self._text = t or ""; self.fs:SetText(self._text) end
  b:SetText(text)
  b:SetScript("OnClick", onClick)
  if EUI_Skin then EUI_Skin.Button(b) end
  return b
end

-- A scrolling list of simple rows. Each refresh hands it an array of row specs:
--   { kind = "header"|"text"|"item"|"bar"|"space", text, right, icon,
--     r, g, b, indent, itemId, bar = { value, min, max, r, g, b, label },
--     onClick, tooltip }
local function rowList(parent, rowWidth)
  local scroll = tryCreate("ScrollFrame", nil, parent, "UIPanelScrollFrameTemplate")
  local child = CreateFrame("Frame", nil, scroll)
  child:SetSize(rowWidth, 1)
  scroll:SetScrollChild(child)
  scroll:EnableMouseWheel(true)
  scroll:SetScript("OnMouseWheel", function(self, delta)
    local max = math.max(0, child:GetHeight() - self:GetHeight())
    self:SetVerticalScroll(math.min(max, math.max(0, self:GetVerticalScroll() - delta * 40)))
  end)

  local rows = {}

  local function rowOnEnter(row)
    local spec = row.spec
    if not spec then return end
    if spec.itemId then
      GameTooltip:SetOwner(row, "ANCHOR_RIGHT")
      if GameTooltip.SetItemByID then GameTooltip:SetItemByID(spec.itemId)
      else GameTooltip:SetHyperlink("item:" .. spec.itemId) end
      GameTooltip:Show()
    elseif spec.tooltip then
      GameTooltip:SetOwner(row, "ANCHOR_RIGHT")
      for i, line in ipairs(spec.tooltip) do
        if i == 1 then GameTooltip:AddLine(line) else GameTooltip:AddLine(line, 0.85, 0.85, 0.85, true) end
      end
      GameTooltip:Show()
    end
  end

  local function rowOnClick(row)
    local spec = row.spec
    if not spec then return end
    if spec.itemId and IsModifiedClick and IsModifiedClick() then
      local link = GetItemInfoCompat and select(2, GetItemInfoCompat(spec.itemId))
      if link and HandleModifiedItemClick then HandleModifiedItemClick(link) end
      return
    end
    if spec.onClick then spec.onClick() end
  end

  local function makeRow()
    local row = CreateFrame("Button", nil, child)
    row:SetWidth(rowWidth)
    row:RegisterForClicks("AnyUp")
    row.icon = row:CreateTexture(nil, "ARTWORK")
    row.icon:SetSize(16, 16)
    row.icon:SetTexCoord(0.07, 0.93, 0.07, 0.93)
    row.text = row:CreateFontString(nil, "OVERLAY")
    row.text:SetFontObject(Fonts.row)
    row.text:SetJustifyH("LEFT")
    row.text:SetWordWrap(false)
    row.right = row:CreateFontString(nil, "OVERLAY")
    row.right:SetFontObject(Fonts.row)
    row.right:SetJustifyH("RIGHT")
    row.right:SetPoint("RIGHT", -4, 0)

    row.subtitle = row:CreateFontString(nil, "OVERLAY")
    row.subtitle:SetFontObject(Fonts.row)
    row.subtitle:SetJustifyH("LEFT")
    row.subtitle:SetWordWrap(false)
    row.rightBottom = row:CreateFontString(nil, "OVERLAY")
    row.rightBottom:SetFontObject(Fonts.row)
    row.rightBottom:SetJustifyH("RIGHT")
    row.rxp = row:CreateFontString(nil, "OVERLAY")
    row.rxp:SetFontObject(Fonts.row)
    row.rxp:SetJustifyH("RIGHT")

    row.restIcon = row:CreateTexture(nil, "OVERLAY")
    row.restIcon:SetSize(14, 14)
    row.restIcon:SetTexture("Interface\\Icons\\Spell_Nature_Sleep")
    row.restIcon:SetTexCoord(0.08, 0.92, 0.08, 0.92)
    row.restIcon:Hide()
    row.mailIcon = row:CreateTexture(nil, "OVERLAY")
    row.mailIcon:SetSize(14, 14)
    row.mailIcon:SetTexture("Interface\\Icons\\INV_Letter_15")
    row.mailIcon:SetTexCoord(0.08, 0.92, 0.08, 0.92)
    row.mailIcon:Hide()

    row.bar = CreateFrame("StatusBar", nil, row)
    row.bar:SetStatusBarTexture("Interface\\TargetingFrame\\UI-StatusBar")
    row.bar:SetHeight(14)
    row.bar.bg = row.bar:CreateTexture(nil, "BACKGROUND")
    row.bar.bg:SetAllPoints()
    row.bar.bg:SetColorTexture(0, 0, 0, 0.5)
    row.bar.label = row.bar:CreateFontString(nil, "OVERLAY")
    row.bar.label:SetFontObject(Fonts.row)
    row.bar.label:SetPoint("CENTER")

    row.sel = row:CreateTexture(nil, "BACKGROUND")
    row.sel:SetAllPoints()
    row.sel:SetColorTexture(THEME.accent[1], THEME.accent[2], THEME.accent[3], 0.12)

    -- Coloured left strip (class colour on the character list).
    row.accent = row:CreateTexture(nil, "ARTWORK")
    row.accent:SetWidth(3)
    row.accent:SetPoint("TOPLEFT", 0, 0)
    row.accent:SetPoint("BOTTOMLEFT", 0, 0)
    row.accent:Hide()

    row.hl = row:CreateTexture(nil, "HIGHLIGHT")
    row.hl:SetAllPoints()
    row.hl:SetColorTexture(1, 1, 1, 0.06)

    row:SetScript("OnEnter", rowOnEnter)
    row:SetScript("OnLeave", function() GameTooltip:Hide() end)
    row:SetScript("OnClick", rowOnClick)
    return row
  end

  local api = {}
  function api.SetRows(specs, keepScroll)
    local y = 0
    for i, spec in ipairs(specs) do
      local row = rows[i] or makeRow()
      rows[i] = row
      row.spec = spec
      local kind = spec.kind or "text"
      local h = Metrics[kind] or Metrics.text
      local indent = (spec.indent or 0) * 14 + 4 + (spec.barColor and 6 or 0)
      row:SetHeight(h)
      row:ClearAllPoints()
      row:SetPoint("TOPLEFT", 0, -y)
      y = y + h

      row.text:SetFontObject(kind == "header" and Fonts.header or Fonts.row)
      row.icon:SetSize(Metrics.icon, Metrics.icon)
      row.bar:SetHeight(Metrics.barHeight)
      row.text:ClearAllPoints()
      row.icon:ClearAllPoints()
      row.subtitle:ClearAllPoints()
      row.rightBottom:ClearAllPoints()
      row.rxp:ClearAllPoints()

      if kind == "character" then
        -- Two-line card: name + class/race on top, level and RXP/gold columns
        -- stacked right. RXP and Gold each anchor their own right edge at a
        -- fixed offset (not to each other) so the two columns stay aligned
        -- down the list regardless of how wide either value's text is.
        row.icon:Hide()
        row.text:SetPoint("TOPLEFT", indent, -4)
        row.text:SetPoint("RIGHT", row, "RIGHT", -62, 0)
        row.subtitle:SetPoint("BOTTOMLEFT", indent, 4)
        row.subtitle:SetText(spec.subtitle or "")
        row.subtitle:SetTextColor(0.6, 0.64, 0.72)
        row.subtitle:Show()
        row.right:ClearAllPoints()
        row.right:SetPoint("TOPRIGHT", -4, -4)
        row.rightBottom:SetPoint("BOTTOMRIGHT", GOLD_COL_X, 4)
        row.rightBottom:SetText(spec.rightBottom or "")
        row.rightBottom:Show()
        row.rxp:SetPoint("BOTTOMRIGHT", RXP_COL_X, 4)
        row.rxp:SetText(spec.rxp or "")
        row.rxp:SetTextColor(0.55, 0.75, 1)
        row.rxp:Show()
      else
        row.subtitle:Hide()
        row.rightBottom:Hide()
        row.rxp:Hide()
        if spec.icon then
          row.icon:SetTexture(spec.icon)
          if spec.iconCoords then row.icon:SetTexCoord(unpack(spec.iconCoords))
          else row.icon:SetTexCoord(0.07, 0.93, 0.07, 0.93) end
          row.icon:SetPoint("LEFT", indent, 0)
          row.icon:Show()
          row.text:SetPoint("LEFT", row.icon, "RIGHT", 5, 0)
        else
          row.icon:Hide()
          row.text:SetPoint("LEFT", indent, 0)
        end
        row.right:ClearAllPoints()
        row.right:SetPoint("RIGHT", -4, 0)
      end
      row.text:SetText(spec.text or "")
      row.text:SetTextColor(spec.r or 1, spec.g or 1, spec.b or 1)
      row.right:SetText(spec.right or "")

      local badgeAnchor = row.right
      local function placeBadge(tex, show)
        if not show then tex:Hide(); return end
        tex:ClearAllPoints()
        tex:SetPoint("RIGHT", badgeAnchor, "LEFT", -4, 0)
        tex:Show()
        badgeAnchor = tex
      end
      placeBadge(row.mailIcon, spec.mail)
      placeBadge(row.restIcon, spec.rested)

      if kind == "bar" and spec.bar then
        local b = spec.bar
        local lo, hi = b.min or 0, b.max or 1
        if hi <= lo then hi = lo + 1 end
        row.bar:ClearAllPoints()
        row.bar:SetPoint("LEFT", Metrics.barLeft, 0)
        row.bar:SetPoint("RIGHT", -4, 0)
        row.bar:SetMinMaxValues(lo, hi)
        row.bar:SetValue(math.min(hi, math.max(lo, b.value or lo)))
        row.bar:SetStatusBarColor(b.r or 0.3, b.g or 0.5, b.b or 0.9)
        row.bar.label:SetText(b.label or "")
        row.bar:Show()
        row.right:Hide()
        row.text:SetPoint("RIGHT", row, "LEFT", Metrics.barLeft - 4, 0)
      else
        row.bar:Hide()
        row.right:Show()
        if kind ~= "character" then row.text:SetPoint("RIGHT", row.right, "LEFT", -8, 0) end
      end
      if spec.selected then row.sel:Show() else row.sel:Hide() end
      if spec.barColor then
        row.accent:SetColorTexture(spec.barColor[1], spec.barColor[2], spec.barColor[3], 0.9)
        row.accent:Show()
      else
        row.accent:Hide()
      end

      local interactive = spec.itemId or spec.onClick or spec.tooltip
      if interactive then row.hl:Show() else row.hl:Hide() end
      row:Show()
    end
    for i = #specs + 1, #rows do rows[i]:Hide(); rows[i].spec = nil end
    child:SetHeight(math.max(1, y))
    if not keepScroll then scroll:SetVerticalScroll(0) end
  end

  api.scroll = scroll
  return api
end

-- A flat bordered "stat card", the overview tab's label-over-value tile.
local function makeTile(parent)
  local t = tryCreate("Frame", nil, parent, "BackdropTemplate")
  if t.SetBackdrop then
    t:SetBackdrop({ bgFile = "Interface\\Buttons\\WHITE8x8", edgeFile = "Interface\\Buttons\\WHITE8x8", edgeSize = 1 })
    t:SetBackdropColor(1, 1, 1, 0.03)
    t:SetBackdropBorderColor(THEME.border[1], THEME.border[2], THEME.border[3], 1)
  end
  t.label = t:CreateFontString(nil, "OVERLAY")
  t.label:SetFontObject(GameFontDisableSmall)
  t.label:SetPoint("TOPLEFT", 10, -8)
  t.label:SetPoint("RIGHT", -10, 0)
  t.label:SetJustifyH("LEFT")
  t.value = t:CreateFontString(nil, "OVERLAY")
  t.value:SetFontObject(Fonts.row)
  t.value:SetPoint("TOPLEFT", t.label, "BOTTOMLEFT", 0, -4)
  t.value:SetPoint("RIGHT", -10, 0)
  t.value:SetJustifyH("LEFT")
  t.value:SetWordWrap(false)
  return t
end

-- A wrapping grid of stat tiles ({label, value} specs), 4 per row.
local function tileGrid(parent)
  local holder = CreateFrame("Frame", nil, parent)
  local tiles = {}
  local api = {}
  function api.SetTiles(items, width)
    local cols, gap, th = 4, 8, 46
    local tw = math.floor((width - gap * (cols - 1)) / cols)
    for i, it in ipairs(items) do
      local tile = tiles[i] or makeTile(holder)
      tiles[i] = tile
      tile:SetSize(tw, th)
      local col, row = (i - 1) % cols, math.floor((i - 1) / cols)
      tile:ClearAllPoints()
      tile:SetPoint("TOPLEFT", col * (tw + gap), -row * (th + gap))
      tile.label:SetText(it.label)
      tile.value:SetText(it.value)
      tile:Show()
    end
    for i = #items + 1, #tiles do tiles[i]:Hide() end
    local rowCount = math.max(1, math.ceil(#items / cols))
    holder:SetSize(width, rowCount * th + (rowCount - 1) * gap)
  end
  api.frame = holder
  return api
end

-- An item card: icon + name + quantity, quality-coloured border. Used by the
-- gear/bags/bank grids. `spec` is an itemRow()-shaped table (icon, text,
-- right, r/g/b, itemId).
local function makeItemTile(parent)
  local t = tryCreate("Button", nil, parent, "BackdropTemplate")
  t:RegisterForClicks("AnyUp")
  if t.SetBackdrop then
    t:SetBackdrop({ bgFile = "Interface\\Buttons\\WHITE8x8", edgeFile = "Interface\\Buttons\\WHITE8x8", edgeSize = 1 })
    t:SetBackdropColor(1, 1, 1, 0.03)
    t:SetBackdropBorderColor(THEME.border[1], THEME.border[2], THEME.border[3], 1)
  end
  t.icon = t:CreateTexture(nil, "ARTWORK")
  t.icon:SetSize(34, 34)
  t.icon:SetPoint("LEFT", 5, 0)
  t.icon:SetTexCoord(0.07, 0.93, 0.07, 0.93)
  t.qty = t:CreateFontString(nil, "OVERLAY")
  t.qty:SetFontObject(Fonts.row)
  t.qty:SetTextColor(1, 1, 1)
  t.qty:SetPoint("BOTTOMRIGHT", t.icon, "BOTTOMRIGHT", 1, -1)
  t.name = t:CreateFontString(nil, "OVERLAY")
  t.name:SetFontObject(Fonts.row)
  t.name:SetPoint("TOPLEFT", t.icon, "TOPRIGHT", 8, -2)
  t.name:SetPoint("RIGHT", -6, 0)
  t.name:SetJustifyH("LEFT")
  t.name:SetWordWrap(false)
  t.hl = t:CreateTexture(nil, "HIGHLIGHT")
  t.hl:SetAllPoints()
  t.hl:SetColorTexture(1, 1, 1, 0.06)
  t:SetScript("OnEnter", function(self)
    local spec = self.spec
    if not spec or not spec.itemId then return end
    GameTooltip:SetOwner(self, "ANCHOR_RIGHT")
    if GameTooltip.SetItemByID then GameTooltip:SetItemByID(spec.itemId) else GameTooltip:SetHyperlink("item:" .. spec.itemId) end
    GameTooltip:Show()
  end)
  t:SetScript("OnLeave", function() GameTooltip:Hide() end)
  t:SetScript("OnClick", function(self)
    local spec = self.spec
    if not spec or not spec.itemId or not (IsModifiedClick and IsModifiedClick()) then return end
    local link = GetItemInfoCompat and select(2, GetItemInfoCompat(spec.itemId))
    if link and HandleModifiedItemClick then HandleModifiedItemClick(link) end
  end)
  return t
end

-- A wrapping grid of item cards, column count fit to width.
local function itemGrid(parent)
  local holder = CreateFrame("Frame", nil, parent)
  local tiles = {}
  local api = {}
  function api.SetItems(items, width)
    local gap, minW, th = 8, 140, 40
    local cols = math.max(1, math.floor((width + gap) / (minW + gap)))
    local tw = math.floor((width - gap * (cols - 1)) / cols)
    for i, it in ipairs(items) do
      local tile = tiles[i] or makeItemTile(holder)
      tiles[i] = tile
      tile.spec = it
      tile:SetSize(tw, th)
      local col, row = (i - 1) % cols, math.floor((i - 1) / cols)
      tile:ClearAllPoints()
      tile:SetPoint("TOPLEFT", col * (tw + gap), -row * (th + gap))
      tile.icon:SetTexture(it.icon)
      local r, g, b = it.r or 1, it.g or 1, it.b or 1
      if tile.SetBackdropBorderColor then tile:SetBackdropBorderColor(r, g, b, 1) end
      tile.name:SetText(it.text or "")
      tile.name:SetTextColor(r, g, b)
      tile.qty:SetText(it.right or "")
      tile:Show()
    end
    for i = #items + 1, #tiles do tiles[i]:Hide() end
    local rows = math.max(1, math.ceil(#items / cols))
    holder:SetSize(width, rows * th + (rows - 1) * gap)
    return holder:GetHeight()
  end
  api.frame = holder
  return api
end

-- A scrolling stack of sections ({title, right, items, empty}), each an item
-- grid under its own header — the gear/bags/bank tabs.
local function sectionList(parent, width)
  local scroll = tryCreate("ScrollFrame", nil, parent, "UIPanelScrollFrameTemplate")
  local child = CreateFrame("Frame", nil, scroll)
  child:SetSize(width, 1)
  scroll:SetScrollChild(child)
  scroll:EnableMouseWheel(true)
  scroll:SetScript("OnMouseWheel", function(self, delta)
    local max = math.max(0, child:GetHeight() - self:GetHeight())
    self:SetVerticalScroll(math.min(max, math.max(0, self:GetVerticalScroll() - delta * 40)))
  end)

  local blocks = {}
  local function makeBlock()
    local b = {}
    b.header = child:CreateFontString(nil, "OVERLAY")
    b.header:SetFontObject(Fonts.header)
    b.header:SetJustifyH("LEFT")
    b.header:SetTextColor(1, 0.82, 0)
    b.right = child:CreateFontString(nil, "OVERLAY")
    b.right:SetFontObject(Fonts.row)
    b.right:SetJustifyH("RIGHT")
    b.right:SetTextColor(0.6, 0.64, 0.72)
    b.empty = child:CreateFontString(nil, "OVERLAY")
    b.empty:SetFontObject(Fonts.row)
    b.empty:SetJustifyH("LEFT")
    b.grid = itemGrid(child)
    return b
  end

  local api = {}
  function api.SetSections(sections, keepScroll)
    local y = 0
    for i, sec in ipairs(sections) do
      local b = blocks[i] or makeBlock()
      blocks[i] = b
      b.header:ClearAllPoints()
      b.header:SetPoint("TOPLEFT", 4, -y)
      b.header:SetText(sec.title or "")
      b.right:ClearAllPoints()
      b.right:SetPoint("TOPRIGHT", -4, -y)
      b.right:SetText(sec.right or "")
      b.header:Show()
      b.right:Show()
      y = y + Metrics.header

      if sec.items and #sec.items > 0 then
        b.empty:Hide()
        b.grid.frame:ClearAllPoints()
        b.grid.frame:SetPoint("TOPLEFT", 4, -y)
        local h = b.grid.SetItems(sec.items, width - 8)
        b.grid.frame:Show()
        y = y + h + 16
      else
        b.grid.frame:Hide()
        b.empty:ClearAllPoints()
        b.empty:SetPoint("TOPLEFT", 8, -y)
        b.empty:SetText(GREY .. (sec.empty or "empty") .. "|r")
        b.empty:Show()
        y = y + Metrics.text + 10
      end
    end
    for i = #sections + 1, #blocks do
      blocks[i].header:Hide(); blocks[i].right:Hide(); blocks[i].empty:Hide(); blocks[i].grid.frame:Hide()
    end
    child:SetHeight(math.max(1, y))
    if not keepScroll then scroll:SetVerticalScroll(0) end
  end

  api.scroll = scroll
  return api
end

-- ---------------------------------------------------------------------------
-- Tabs: each returns row specs for the selected character
-- ---------------------------------------------------------------------------
local Tabs = {}

local function header(text, right) return { kind = "header", text = text, right = right, r = 1, g = 0.82, b = 0 } end
local function line(label, value, tooltip)
  return { kind = "text", text = GREY .. label .. "|r", right = value, tooltip = tooltip }
end
local function bar(label, value, lo, hi, text, r, g, b, tooltip)
  return { kind = "bar", text = label, bar = { value = value, min = lo, max = hi, label = text, r = r, g = g, b = b }, tooltip = tooltip }
end
local function itemRow(item, right, indent)
  local r, g, b = qualityColor(item.quality)
  return { kind = "item", icon = itemIcon(item), text = item.name or ("Item " .. tostring(item.id)),
           right = right, r = r, g = g, b = b, itemId = item.id, indent = indent }
end

-- Stat-card grid shown above the overview tab's row list.
local function overviewTiles(c)
  local used, total = 0, 0
  for _, bag in ipairs(c.bags or {}) do total = total + (bag.size or 0); used = used + (bag.size or 0) - (bag.free or 0) end
  local bagText = GREY .. "—|r"
  if total > 0 then
    local free = total - used
    local col = free == 0 and "|cffff4040" or free < 6 and "|cffffa040" or WHITE
    bagText = ("%s%d|r free of %d"):format(col, free, total)
  end
  local mailText = GREY .. "—|r"
  if c.mail then
    mailText = c.mail.count == 0 and (GREY .. "None|r") or (WHITE .. c.mail.count .. " item" .. (c.mail.count == 1 and "" or "s") .. "|r")
  end
  return {
    { label = "Location", value = c.zone and (c.zone .. ((c.subZone and c.subZone ~= "") and (", " .. c.subZone) or "")) or GREY .. "Unknown|r" },
    { label = "Hearthstone", value = c.bindLocation or GREY .. "—|r" },
    { label = "Time played", value = c.played and duration(c.played) or GREY .. "—|r" },
    { label = "Item level", value = c.itemLevel and ("%.1f"):format(c.itemLevel) or GREY .. "—|r" },
    { label = "Gold", value = money(c.money) },
    { label = "Bag space", value = bagText },
    { label = "Bank", value = c.bankScanned and ("saved " .. ago(c.bankScanned)) or (GREY .. "not recorded|r") },
    { label = "Mail", value = mailText },
  }
end

-- The name/level/XP header is drawn once above the tabs (see progressBar in
-- build()); the overview tab itself starts with the mailbox row.
Tabs.overview = function(c)
  local rows = {}
  local isMe = c.guid == API.GetCurrentGUID()
  if c.mail and c.mail.soonestExpiry and c.mail.count > 0 then
    local left = c.mail.soonestExpiry - time()
    local col = left < 3 * 86400 and "|cffff4040" or GREY
    local text = left > 0 and ("first expires in " .. duration(left)) or "expired"
    rows[#rows + 1] = line("Mailbox", col .. text .. "|r",
      { "Mailbox", "As of the last time this character opened the mailbox (" .. ago(c.mail.scanned) .. ")." })
  end

  if not isMe then
    rows[#rows + 1] = { kind = "space" }
    rows[#rows + 1] = { kind = "text", text = GREY .. "Forget this character…|r",
      tooltip = { "Forget this character", "Removes it from RosterBook, e.g. after deleting or transferring it. Logging in on it records it again." },
      onClick = function() StaticPopup_Show("ROSTERBOOK_FORGET", c.name, nil, c.guid) end }
  end
  return rows
end

-- Tree: profession → (click) its recipes → (click a recipe) its reagents.
-- Expand state is per character + per profession/recipe, and not saved —
-- it's view state, not a setting, and resets to all-collapsed next time the
-- panel opens.
local openProfessions, openRecipes = {}, {}
Tabs.professions = function(c)
  local rows = {}
  local groups = { { "primary", "Professions" }, { "secondary", "Secondary skills" } }
  for _, g in ipairs(groups) do
    local first = true
    for _, p in ipairs(c.professions or {}) do
      if p.kind == g[1] then
        if first then rows[#rows + 1] = header(g[2]); first = false else rows[#rows + 1] = { kind = "space" } end
        local profKey = c.guid .. "|" .. p.name
        local profOpen = openProfessions[profKey]
        rows[#rows + 1] = bar((profOpen and "v  " or ">  ") .. p.name, p.rank or 0, 0, p.max or 1,
          ("%d / %d"):format(p.rank or 0, p.max or 0), 0.2, 0.55, 0.9, p.expansion and { p.name, p.expansion } or nil)
        rows[#rows].onClick = function() openProfessions[profKey] = not profOpen; Panel.Refresh(true) end

        if profOpen then
          local recipes = {}
          for _, r in ipairs(c.recipes or {}) do if r.profession == p.name then recipes[#recipes + 1] = r end end
          table.sort(recipes, function(a, b) return (a.name or "") < (b.name or "") end)
          if #recipes > 0 then
            for _, r in ipairs(recipes) do
              local recipeKey = c.guid .. "|" .. r.id
              local recipeOpen = openRecipes[recipeKey]
              local n = #(r.reagents or {})
              rows[#rows + 1] = {
                kind = "item", icon = itemIcon({ id = r.output }), itemId = r.output,
                text = (recipeOpen and "v  " or ">  ") .. r.name,
                right = GREY .. n .. " reagent" .. (n == 1 and "" or "s") .. "|r", indent = 1,
                onClick = function() openRecipes[recipeKey] = not recipeOpen; Panel.Refresh(true) end,
              }
              if recipeOpen then
                if n > 0 then
                  for _, rg in ipairs(r.reagents) do
                    rows[#rows + 1] = itemRow({ id = rg.id, name = rg.name or ("Item #" .. rg.id), quality = 1 },
                      WHITE .. rg.count .. "x|r", 2)
                  end
                else
                  rows[#rows + 1] = { kind = "text", text = GREY .. "No reagents.|r", indent = 2 }
                end
              end
            end
          else
            rows[#rows + 1] = { kind = "text", text = GREY .. "Open this profession once to record its recipes.|r", indent = 1 }
          end
        end
      end
    end
  end
  if #rows == 0 then rows[1] = line("No professions learned yet.", "") end
  return rows
end

-- Gear/bags/bank tabs return a list of sections ({title, right, items, empty})
-- for the sectionList grid rather than flat row specs.
Tabs.gear = function(c)
  local gear = {}
  for _, it in ipairs(c.equipment or {}) do gear[#gear + 1] = it end
  table.sort(gear, function(a, b) return (a.slot or 0) < (b.slot or 0) end)
  local items = {}
  for _, it in ipairs(gear) do items[#items + 1] = itemRow(it, GREY .. slotName(it.slot) .. "|r") end
  return { { title = "Equipped", right = c.itemLevel and ("item level %.1f"):format(c.itemLevel) or nil,
             items = items, empty = "Nothing recorded yet." } }
end

-- One section per bag, with stacks of the same item added together.
local function bagSections(sections, bags)
  for _, bag in ipairs(bags) do
    local free = bag.free or 0
    local name = bag.name or (bag.id == 0 and (BACKPACK_TOOLTIP or "Backpack")) or "Bag"
    local merged, order = {}, {}
    for _, it in ipairs(bag.items or {}) do
      local key = it.id or it.name
      if merged[key] then merged[key].count = merged[key].count + (it.count or 1)
      else
        merged[key] = { id = it.id, name = it.name, quality = it.quality, icon = it.icon, count = it.count or 1 }
        order[#order + 1] = key
      end
    end
    local items = {}
    for _, key in ipairs(order) do
      local it = merged[key]
      items[#items + 1] = itemRow(it, it.count > 1 and ("x" .. it.count) or nil)
    end
    sections[#sections + 1] = { title = name, right = GREY .. (bag.size - free) .. "/" .. bag.size .. "|r",
                                 items = items, empty = "empty" }
  end
end

Tabs.bags = function(c)
  local bags = c.bags or {}
  if #bags == 0 then return { { title = "Bags", empty = "Nothing recorded yet." } } end
  local sections = {}
  bagSections(sections, bags)
  return sections
end

Tabs.bank = function(c)
  local sections = {}
  if c.bank and #c.bank > 0 then
    bagSections(sections, c.bank)
  else
    sections[1] = { title = "Bank", empty = "Visit a banker on this character to record the bank." }
  end
  local wb = API.GetDB().warband
  if wb and wb.tabs and #wb.tabs > 0 then
    sections[#sections + 1] = { title = "Warband bank", right = wb.money and money(wb.money) or nil, empty = "" }
    bagSections(sections, wb.tabs)
  end
  return sections
end

Tabs.reputation = function(c)
  local rows, group = {}, nil
  for _, rep in ipairs(c.reputations or {}) do
    if rep.group ~= group then
      group = rep.group
      -- A header that has its own reputation (e.g. "Alliance") is shown as
      -- that bar instead of a separate heading.
      if rep.name ~= group then rows[#rows + 1] = header(group or "Other") end
    end
    local label, r, g, b
    if rep.kind == "renown" or rep.kind == "friendship" then
      label, r, g, b = rep.label or "", 0.0, 0.6, 0.85
    else
      label = _G["FACTION_STANDING_LABEL" .. (rep.standing or 4)] or ""
      local col = FACTION_BAR_COLORS and FACTION_BAR_COLORS[rep.standing or 4]
      r, g, b = col and col.r or 0.4, col and col.g or 0.6, col and col.b or 0.2
    end
    local lo, hi, v = rep.min or 0, rep.max or 0, rep.value or 0
    local maxed = rep.kind == "standing" and (rep.standing or 0) >= 8
    local text = (hi > lo and not maxed) and ("%s  %d / %d"):format(label, v - lo, hi - lo) or label
    rows[#rows + 1] = bar(rep.name, v, lo, hi > lo and hi or v + 1, text, r, g, b)
    rows[#rows].indent = rep.name == group and 0 or 1
  end
  if #rows == 0 then rows[1] = line("No reputations recorded.", "") end
  return rows
end

Tabs.currency = function(c)
  local rows = {}
  for _, cur in ipairs(c.currencies or {}) do
    local right = WHITE .. commas(cur.quantity) .. "|r"
    if cur.maxQuantity and cur.maxQuantity > 0 then
      right = right .. GREY .. " / " .. commas(cur.maxQuantity) .. "|r"
    end
    rows[#rows + 1] = itemRow(cur, right)
  end
  if #rows == 0 then rows[1] = line("No currencies recorded.", "") end
  return rows
end

-- Cross-character item search: every character's gear, bags and bank, plus
-- the Warband bank, matched by item name.
local function findItems(query)
  local rows = {}
  query = (query or ""):lower():match("^%s*(.-)%s*$")
  if #query < 2 then
    return { { kind = "text", text = GREY .. "Type at least two letters of an item name.|r" } }
  end
  local found, order = {}, {}
  local function add(item, owner, where)
    if not item.name or not item.name:lower():find(query, 1, true) then return end
    local key = item.id or item.name
    local f = found[key]
    if not f then
      f = { item = item, total = 0, owners = {}, ownerOrder = {} }
      found[key] = f
      order[#order + 1] = key
    end
    local o = f.owners[owner.key]
    if not o then
      o = { owner = owner, places = {}, placeOrder = {} }
      f.owners[owner.key] = o
      f.ownerOrder[#f.ownerOrder + 1] = owner.key
    end
    if not o.places[where] then o.placeOrder[#o.placeOrder + 1] = where end
    o.places[where] = (o.places[where] or 0) + (item.count or 1)
    f.total = f.total + (item.count or 1)
  end

  for _, c in ipairs(characters()) do
    local _, _, _, hex = classColor(c.classToken)
    local owner = { key = c.guid, label = "|c" .. hex .. (c.name or "?") .. "|r" }
    for _, it in ipairs(c.equipment or {}) do add(it, owner, "equipped") end
    for _, bag in ipairs(c.bags or {}) do for _, it in ipairs(bag.items or {}) do add(it, owner, "bags") end end
    for _, bag in ipairs(c.bank or {}) do for _, it in ipairs(bag.items or {}) do add(it, owner, "bank") end end
  end
  local wb = API.GetDB().warband
  if wb and wb.tabs then
    local owner = { key = "warband", label = "|cff66ccffWarband bank|r" }
    for _, tab in ipairs(wb.tabs) do for _, it in ipairs(tab.items or {}) do add(it, owner, "bank") end end
  end

  table.sort(order, function(a, b) return (found[a].item.name or "") < (found[b].item.name or "") end)
  for i, key in ipairs(order) do
    if i > 60 then
      rows[#rows + 1] = { kind = "text", text = GREY .. (#order - 60) .. " more — type more of the name to narrow it down.|r" }
      break
    end
    local f = found[key]
    rows[#rows + 1] = itemRow(f.item, WHITE .. f.total .. "|r")
    for _, ok in ipairs(f.ownerOrder) do
      local o = f.owners[ok]
      local parts = {}
      for _, where in ipairs(o.placeOrder) do parts[#parts + 1] = where .. " " .. o.places[where] end
      rows[#rows + 1] = { kind = "text", text = o.owner.label, right = GREY .. table.concat(parts, ", ") .. "|r", indent = 2 }
    end
  end
  if #order == 0 then rows[1] = { kind = "text", text = GREY .. "No items match \"" .. query:gsub("|", "||") .. "\".|r" } end
  return rows
end

local TAB_ORDER = {
  { key = "overview", label = "Overview" },
  { key = "bags", label = "Bags" },
  { key = "bank", label = "Bank" },
  { key = "gear", label = "Gear" },
  { key = "professions", label = "Professions" },
  { key = "reputation", label = "Reputation" },
  { key = "currency", label = "Currency" },
}

-- ---------------------------------------------------------------------------
-- Window
-- ---------------------------------------------------------------------------
local WIDTH, HEIGHT, LIST_W = 800, 480, 236
-- Right-pane vertical layout: name/gold, then subtitle/last-seen, then the
-- level/XP bar, then the tab row, then each tab's content — same order as
-- the desktop app.
local TITLE_TOP, TABS_TOP, CONTENT_TOP = -44, -132, -166
local frame, charList, detail, detailGrid, detailTitle, detailSub, detailGold, detailLastSeen, progressBar,
      tabButtons, findBox, findHint, sortButton, totalText, overviewGrid
local settingsView, settingsButton
local selected
local lastTab = "overview"

local function selectedCharacter(list)
  for _, c in ipairs(list) do if c.guid == selected then return c end end
  selected = API.GetCurrentGUID()
  for _, c in ipairs(list) do if c.guid == selected then return c end end
  return list[1]
end

-- The persistent level/XP header shown above the tabs, for every tab.
local function updateProgressBar(c)
  if c.maxLevel or c.xpDisabled or not c.xpMax or c.xpMax == 0 then
    progressBar.bar:Hide()
    progressBar.left:SetText("Level " .. (c.level or "?") .. (c.xpDisabled and GREY .. "  (XP off)|r" or ""))
    progressBar.right:SetText("")
  else
    progressBar.bar:Show()
    progressBar.bar:SetMinMaxValues(0, c.xpMax)
    progressBar.bar:SetValue(math.min(c.xpMax, math.max(0, c.xp or 0)))
    progressBar.bar:SetStatusBarColor(THEME.accent[1], THEME.accent[2], THEME.accent[3])
    progressBar.left:SetText(("%d / %d XP (%d%%)"):format(c.xp or 0, c.xpMax, (c.xp or 0) * 100 / c.xpMax))
    progressBar.right:SetText((c.restXP or 0) > 0 and (c.restXP .. " rested") or "")
  end
end

function Panel.Refresh(keepScroll)
  if not frame or not frame:IsShown() then return end
  local list = characters()
  local current = selectedCharacter(list)
  selected = current and current.guid

  -- Character list
  local specs, total = {}, 0
  for _, c in ipairs(list) do
    total = total + (c.money or 0)
    local r, g, b = classColor(c.classToken)
    local isMe = c.guid == API.GetCurrentGUID()
    local tooltip = { c.name, ("Level %s %s %s"):format(c.level or "?", c.race or "", c.class or ""),
                       (c.realm or "") .. (c.faction and (" — " .. c.faction) or ""),
                       isMe and "Playing now" or ("Last played " .. ago(lastPlayed(c))) }
    if hasRestBonus(c) then tooltip[#tooltip + 1] = "+" .. c.restXP .. " rested XP" end
    if hasMail(c) then
      tooltip[#tooltip + 1] = c.mail.count .. " item" .. (c.mail.count == 1 and "" or "s") .. " in the mailbox"
    end
    specs[#specs + 1] = {
      kind = "character", text = c.name .. (isMe and GREY .. " (you)|r" or ""), r = r, g = g, b = b,
      subtitle = GREY .. (c.race or "") .. " " .. (c.class or "") .. "|r",
      right = WHITE .. (c.level or "?") .. "|r", rightBottom = goldShort(c.money), rxp = restPctText(c),
      selected = c.guid == selected, barColor = { r, g, b },
      rested = hasRestBonus(c), mail = hasMail(c),
      onClick = function()
        selected = c.guid; cfg.selected = c.guid
        if cfg.tab == "settings" then cfg.tab = lastTab end
        Panel.Refresh()
      end,
      tooltip = tooltip,
    }
  end
  charList.SetRows(specs, true)

  local wb = API.GetDB().warband
  local wbText = (wb and wb.money and wb.money > 0) and ("\n" .. GREY .. "Warband bank|r  " .. money(wb.money)) or ""
  totalText:SetText(("%d character%s  " .. money(total)):format(#list, #list == 1 and "" or "s") .. wbText)
  sortButton:SetText("Sort: " .. sortSpec().label)

  -- Detail pane
  for _, b in pairs(tabButtons) do
    if cfg.tab == "settings" then
      b:Hide()
    else
      b:Show()
      local label = b.label
      if b.key == "bags" and current then
        local used, bagTotal = bagUsedTotal(current)
        if bagTotal > 0 then label = label .. "  " .. GREY .. used .. "/" .. bagTotal .. "|r" end
      end
      b.fs:SetText(label)
      b:SetWidth(math.max(48, b.fs:GetStringWidth() + 16))
      if b.key == cfg.tab then b.fs:SetTextColor(1, 1, 1); b.selected:Show() else b.fs:SetTextColor(0.6, 0.64, 0.72); b.selected:Hide() end
      if EUI_Skin and EUI_Skin.SetTabSelection then EUI_Skin.SetTabSelection(b, b.key == cfg.tab) end
    end
  end
  if cfg.tab == "settings" then
    overviewGrid.frame:Hide()
    detail.scroll:Hide()
    detailGrid.scroll:Hide()
    progressBar:Hide()
    detailGold:SetText("")
    detailLastSeen:SetText("")
    settingsView:Show()
    settingsView.Sync()
    detailTitle:SetText("Settings")
    detailSub:SetText(GREY .. "Saved for every character on this account.|r")
    return
  end
  settingsView:Hide()

  -- The search bar is a standing top-bar field, not a tab: typing in it
  -- overrides whatever tab is open, same as the desktop app.
  local query = findBox:GetText()
  if query ~= "" then
    for _, b in pairs(tabButtons) do b.fs:SetTextColor(0.6, 0.64, 0.72); b.selected:Hide() end
    overviewGrid.frame:Hide()
    detailGrid.scroll:Hide()
    progressBar:Hide()
    detailGold:SetText("")
    detailLastSeen:SetText("")
    detail.scroll:Show()
    detailTitle:SetText("Find an item")
    detailSub:SetText(GREY .. "Across every character's gear, bags and bank" .. (wb and ", and the Warband bank" or "") .. ". Shift-click an item to link it.|r")
    detail.scroll:ClearAllPoints()
    detail.scroll:SetPoint("TOPLEFT", frame, "TOPLEFT", LIST_W + 28, CONTENT_TOP)
    detail.scroll:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -32, 14)
    detail.SetRows(findItems(query), keepScroll)
    return
  end

  if not current then
    overviewGrid.frame:Hide()
    detailGrid.scroll:Hide()
    progressBar:Hide()
    detailGold:SetText("")
    detailLastSeen:SetText("")
    detail.scroll:Show()
    detail.scroll:ClearAllPoints()
    detail.scroll:SetPoint("TOPLEFT", frame, "TOPLEFT", LIST_W + 28, CONTENT_TOP)
    detail.scroll:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -32, 14)
    detailTitle:SetText("No characters yet")
    detailSub:SetText("")
    detail.SetRows({ line("Log in on each character once to add it.", "") })
    return
  end

  progressBar:Show()
  updateProgressBar(current)
  detailGold:SetText(money(current.money))
  detailLastSeen:SetText(GREY .. "Last seen " .. (current.guid == API.GetCurrentGUID() and "now" or ago(lastPlayed(current))) .. "|r")

  local GRID_TABS = { gear = true, bags = true, bank = true }
  if GRID_TABS[cfg.tab] then
    overviewGrid.frame:Hide()
    detail.scroll:Hide()
    detailGrid.scroll:Show()
    detailGrid.scroll:ClearAllPoints()
    detailGrid.scroll:SetPoint("TOPLEFT", frame, "TOPLEFT", LIST_W + 28, CONTENT_TOP)
    detailGrid.scroll:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -32, 14)
    detailGrid.SetSections(Tabs[cfg.tab](current), keepScroll)
  elseif cfg.tab == "overview" then
    detailGrid.scroll:Hide()
    detail.scroll:Show()
    overviewGrid.SetTiles(overviewTiles(current), WIDTH - LIST_W - 60)
    overviewGrid.frame:Show()
    detail.scroll:ClearAllPoints()
    detail.scroll:SetPoint("TOPLEFT", overviewGrid.frame, "BOTTOMLEFT", 0, -14)
    detail.scroll:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -32, 14)
    detail.SetRows(Tabs.overview(current), keepScroll)
  else
    detailGrid.scroll:Hide()
    overviewGrid.frame:Hide()
    detail.scroll:Show()
    detail.scroll:ClearAllPoints()
    detail.scroll:SetPoint("TOPLEFT", frame, "TOPLEFT", LIST_W + 28, CONTENT_TOP)
    detail.scroll:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -32, 14)
    detail.SetRows((Tabs[cfg.tab] or Tabs.overview)(current), keepScroll)
  end

  local _, _, _, hex = classColor(current.classToken)
  detailTitle:SetText("|c" .. hex .. current.name .. "|r")
  detailSub:SetText(("Level %s %s %s  " .. GREY .. "%s%s|r"):format(current.level or "?", current.race or "", current.class or "",
    current.realm or "", current.faction and (" — " .. current.faction) or ""))
end

-- ---------------------------------------------------------------------------
-- Settings view
-- ---------------------------------------------------------------------------
local STRATA = {
  { key = "MEDIUM", label = "Normal" },
  { key = "HIGH", label = "Above most windows" },
  { key = "DIALOG", label = "On top of everything" },
}

-- Names registered with LibSharedMedia for the given media type (e.g. other
-- UI add-ons' border or font packs), alphabetical, with `none` first.
local function mediaList(mtype, none)
  local list = { none }
  if LSM then
    local names = {}
    for name in pairs(LSM:HashTable(mtype)) do names[#names + 1] = name end
    table.sort(names)
    for _, name in ipairs(names) do list[#list + 1] = name end
  end
  return list
end

-- A scrollable dropdown: a toggle button plus a floating list popup, for
-- picking one of a (possibly long) list of names, e.g. shared-media borders
-- or fonts. `getList` is called fresh each time the popup opens.
local DROPDOWN_ROW_H = 20
local openDropdown
local function dropdown(parent, width, visibleRows, getList, onSelect)
  local toggle = button(parent, "", width, nil)
  toggle.fs:ClearAllPoints()
  toggle.fs:SetPoint("LEFT", 8, 0)
  toggle.fs:SetPoint("RIGHT", -18, 0)
  toggle.fs:SetJustifyH("LEFT")
  local arrow = toggle:CreateFontString(nil, "OVERLAY")
  arrow:SetFontObject(Fonts.row)
  arrow:SetPoint("RIGHT", -6, 0)
  arrow:SetText(GREY .. "v|r")

  local popup = CreateFrame("Frame", nil, UIParent, "BackdropTemplate")
  popup:SetFrameStrata("TOOLTIP")
  popup:SetWidth(width)
  popup:SetHeight(visibleRows * DROPDOWN_ROW_H + 8)
  popup:Hide()
  if popup.SetBackdrop then
    popup:SetBackdrop({ bgFile = "Interface\\Buttons\\WHITE8x8", edgeFile = "Interface\\Buttons\\WHITE8x8", edgeSize = 1 })
    popup:SetBackdropColor(THEME.bg[1], THEME.bg[2], THEME.bg[3], 0.98)
    popup:SetBackdropBorderColor(THEME.border[1], THEME.border[2], THEME.border[3], 1)
  end
  if EUI_Skin then EUI_Skin.Panel(popup) end

  local catcher = CreateFrame("Button", nil, UIParent)
  catcher:SetAllPoints(UIParent)
  catcher:SetFrameStrata("FULLSCREEN_DIALOG")
  catcher:Hide()
  catcher:SetScript("OnClick", function() popup:Hide() end)
  popup:SetScript("OnHide", function() catcher:Hide(); if openDropdown == popup then openDropdown = nil end end)

  local scroll = tryCreate("ScrollFrame", nil, popup, "UIPanelScrollFrameTemplate")
  scroll:SetPoint("TOPLEFT", 4, -4)
  scroll:SetPoint("BOTTOMRIGHT", -24, 4)
  local child = CreateFrame("Frame", nil, scroll)
  child:SetSize(width - 30, 1)
  scroll:SetScrollChild(child)
  scroll:EnableMouseWheel(true)
  scroll:SetScript("OnMouseWheel", function(self, delta)
    local max = math.max(0, child:GetHeight() - self:GetHeight())
    self:SetVerticalScroll(math.min(max, math.max(0, self:GetVerticalScroll() - delta * (DROPDOWN_ROW_H * 2))))
  end)
  if EUI_Skin and scroll.ScrollBar then EUI_Skin.ScrollBar(scroll.ScrollBar) end

  local optionButtons = {}
  local function rebuild()
    local list = getList()
    for _, b in ipairs(optionButtons) do b:Hide() end
    for i, name in ipairs(list) do
      local b = optionButtons[i]
      if not b then
        b = CreateFrame("Button", nil, child)
        b:SetHeight(DROPDOWN_ROW_H)
        b.hl = b:CreateTexture(nil, "HIGHLIGHT")
        b.hl:SetAllPoints()
        b.hl:SetColorTexture(1, 1, 1, 0.08)
        b.fs = b:CreateFontString(nil, "OVERLAY")
        b.fs:SetFontObject(Fonts.row)
        b.fs:SetPoint("LEFT", 4, 0)
        b.fs:SetPoint("RIGHT", -4, 0)
        b.fs:SetJustifyH("LEFT")
        b.fs:SetWordWrap(false)
        optionButtons[i] = b
      end
      b:SetWidth(child:GetWidth())
      b:SetPoint("TOPLEFT", 0, -(i - 1) * DROPDOWN_ROW_H)
      b.fs:SetText(name)
      b:SetScript("OnClick", function()
        onSelect(name)
        popup:Hide()
      end)
      b:Show()
    end
    child:SetHeight(math.max(1, #list * DROPDOWN_ROW_H))
  end

  toggle:SetScript("OnClick", function()
    if popup:IsShown() then popup:Hide(); return end
    if openDropdown and openDropdown ~= popup then openDropdown:Hide() end
    rebuild()
    popup:ClearAllPoints()
    popup:SetPoint("TOPLEFT", toggle, "BOTTOMLEFT", 0, -2)
    popup:Show()
    catcher:Show()
    openDropdown = popup
  end)

  return toggle
end

local function applySettings()
  local o = cfg.settings
  -- An explicit LibSharedMedia pick always wins; otherwise match EllesmereUI's
  -- own font when it's skinning us, else the game's default.
  local fontPath = (LSM and o.fontStyle ~= "Default" and LSM:Fetch("font", o.fontStyle))
    or (EUI_Skin and EUI_Skin.GetFont())
    or nil
  applyFonts(o.fontSize, fontPath)
  if not frame then return end
  -- EllesmereUI already painted the window's backdrop/border via S.Shell;
  -- don't fight it with our own flat or LibSharedMedia border.
  if frame.SetBackdrop and not EUI_Skin then
    if LSM and o.borderStyle ~= "None" then
      frame:SetBackdrop({
        bgFile = "Interface\\Buttons\\WHITE8x8", edgeFile = LSM:Fetch("border", o.borderStyle),
        edgeSize = 16, insets = { left = 4, right = 4, top = 4, bottom = 4 },
      })
      frame:SetBackdropBorderColor(1, 1, 1, 1)
    else
      frame:SetBackdrop({ bgFile = "Interface\\Buttons\\WHITE8x8", edgeFile = "Interface\\Buttons\\WHITE8x8", edgeSize = 1 })
      frame:SetBackdropBorderColor(THEME.border[1], THEME.border[2], THEME.border[3], 1)
    end
  end
  if frame.SetBackdropColor and not EUI_Skin then frame:SetBackdropColor(0.05, 0.05, 0.07, o.opacity) end
  frame:SetScale(o.scale)
  frame:SetFrameStrata(o.strata)
  frame:SetClampedToScreen(o.clamp)
  for i = #UISpecialFrames, 1, -1 do
    if UISpecialFrames[i] == "RosterbookPanel" then table.remove(UISpecialFrames, i) end
  end
  if o.escape then tinsert(UISpecialFrames, "RosterbookPanel") end
end

local function showTooltip(owner, title, text)
  GameTooltip:SetOwner(owner, "ANCHOR_RIGHT")
  GameTooltip:AddLine(title)
  GameTooltip:AddLine(text, 0.85, 0.85, 0.85, true)
  GameTooltip:Show()
end

-- A plain horizontal slider; built by hand so it looks the same on every client.
local function slider(parent, label, tip, lo, hi, step, format, get, set, onRelease)
  local holder = CreateFrame("Frame", nil, parent)
  holder:SetSize(220, 44)
  holder.label = holder:CreateFontString(nil, "OVERLAY")
  holder.label:SetFontObject(GameFontHighlightSmall)
  holder.label:SetPoint("TOPLEFT", 2, 0)

  local s = tryCreate("Slider", nil, holder, "BackdropTemplate")
  s:SetOrientation("HORIZONTAL")
  s:SetSize(210, 16)
  s:SetPoint("TOPLEFT", 0, -16)
  if s.SetBackdrop then
    s:SetBackdrop({
      bgFile = "Interface\\Buttons\\UI-SliderBar-Background",
      edgeFile = "Interface\\Buttons\\UI-SliderBar-Border",
      tile = true, tileSize = 8, edgeSize = 8,
      insets = { left = 3, right = 3, top = 6, bottom = 6 },
    })
  end
  s:SetThumbTexture("Interface\\Buttons\\UI-SliderBar-Button-Horizontal")
  s:SetMinMaxValues(lo, hi)
  s:SetValueStep(step)
  if s.SetObeyStepOnDrag then s:SetObeyStepOnDrag(true) end
  s:EnableMouseWheel(true)

  local function round(v) return math.floor(v / step + 0.5) * step end
  local function show(v) holder.label:SetText(label .. ":  " .. WHITE .. format(v) .. "|r") end
  local syncing = false
  s:SetScript("OnValueChanged", function(_, v)
    v = round(v)
    show(v)
    if syncing or v == get() then return end
    set(v)
    if not onRelease then applySettings(); Panel.Refresh(true) end
  end)
  -- Window scale is applied when you let go, so the slider doesn't move
  -- out from under the cursor while you drag it.
  if onRelease then
    s:SetScript("OnMouseUp", function() applySettings(); Panel.Refresh(true) end)
  end
  s:SetScript("OnMouseWheel", function(self, delta)
    self:SetValue(math.min(hi, math.max(lo, round(self:GetValue() + delta * step))))
    if onRelease then applySettings(); Panel.Refresh(true) end
  end)
  s:SetScript("OnEnter", function(self) showTooltip(self, label, tip) end)
  s:SetScript("OnLeave", function() GameTooltip:Hide() end)

  function holder.Sync()
    syncing = true
    s:SetValue(get())
    show(get())
    syncing = false
  end
  holder.slider = s
  return holder
end

local function checkbox(parent, label, tip, get, set)
  local cb = CreateFrame("CheckButton", nil, parent)
  cb:SetSize(24, 24)
  cb:SetNormalTexture("Interface\\Buttons\\UI-CheckBox-Up")
  cb:SetPushedTexture("Interface\\Buttons\\UI-CheckBox-Down")
  cb:SetHighlightTexture("Interface\\Buttons\\UI-CheckBox-Highlight", "ADD")
  cb:SetCheckedTexture("Interface\\Buttons\\UI-CheckBox-Check")
  cb.label = cb:CreateFontString(nil, "OVERLAY")
  cb.label:SetFontObject(GameFontHighlightSmall)
  cb.label:SetPoint("LEFT", cb, "RIGHT", 2, 1)
  cb.label:SetText(label)
  cb:SetScript("OnClick", function(self)
    set(self:GetChecked() and true or false)
    applySettings()
    Panel.Refresh(true)
  end)
  cb:SetScript("OnEnter", function(self) showTooltip(self, label, tip) end)
  cb:SetScript("OnLeave", function() GameTooltip:Hide() end)
  function cb.Sync() cb:SetChecked(get() and true or false) end
  if EUI_Skin then EUI_Skin.Checkbox(cb) end
  return cb
end

local function buildSettingsView()
  local v = CreateFrame("Frame", nil, frame)
  v:SetPoint("TOPLEFT", frame, "TOPLEFT", LIST_W + 30, -110)
  v:SetPoint("BOTTOMRIGHT", frame, "BOTTOMRIGHT", -20, 14)
  v:Hide()
  local o = function() return cfg.settings end
  local controls = {}
  local function add(c) controls[#controls + 1] = c; return c end

  local function heading(text, x, y)
    local h = v:CreateFontString(nil, "OVERLAY")
    h:SetFontObject(GameFontNormal)
    h:SetPoint("TOPLEFT", x, y)
    h:SetText(text)
  end

  -- Left column: how the window looks
  heading("Appearance", 0, 0)
  add(slider(v, "Background opacity", "How see-through the window's background is. Text and borders stay solid.",
    0, 1, 0.05, function(x) return ("%d%%"):format(x * 100 + 0.5) end,
    function() return o().opacity end, function(x) o().opacity = x end)):SetPoint("TOPLEFT", 0, -24)
  add(slider(v, "Text size", "Size of the text in the character list and tabs.",
    8, 18, 1, function(x) return x .. (x == BASE_FONT_SIZE and GREY .. "  (game default)|r" or "") end,
    function() return o().fontSize end, function(x) o().fontSize = x end)):SetPoint("TOPLEFT", 0, -72)
  add(slider(v, "Window scale", "Makes the whole window bigger or smaller.",
    0.6, 1.5, 0.05, function(x) return ("%d%%"):format(x * 100 + 0.5) end,
    function() return o().scale end, function(x) o().scale = x end, true)):SetPoint("TOPLEFT", 0, -120)

  local layer = button(v, "", 210, function()
    local idx = 1
    for i, st in ipairs(STRATA) do if st.key == o().strata then idx = i end end
    o().strata = STRATA[idx % #STRATA + 1].key
    applySettings()
    v.Sync()
  end)
  layer:SetPoint("TOPLEFT", 0, -186)
  layer:SetScript("OnEnter", function(self)
    showTooltip(self, "Window layer", "Whether the window sits above or below other windows such as your bags and the map.")
  end)
  layer:SetScript("OnLeave", function() GameTooltip:Hide() end)
  local layerLabel = v:CreateFontString(nil, "OVERLAY")
  layerLabel:SetFontObject(GameFontHighlightSmall)
  layerLabel:SetPoint("BOTTOMLEFT", layer, "TOPLEFT", 2, 3)
  layerLabel:SetText("Window layer")
  add({ Sync = function()
    for _, st in ipairs(STRATA) do if st.key == o().strata then layer:SetText(st.label) end end
  end })

  -- Border and font come from LibSharedMedia, so any media pack another
  -- add-on (ElvUI, Ellesmere UI, a standalone shared-media library, ...)
  -- registers shows up here too — no per-add-on skin support needed.
  local border = dropdown(v, 210, 10, function() return mediaList("border", "None") end, function(name)
    o().borderStyle = name
    applySettings()
    v.Sync()
  end)
  border:SetPoint("TOPLEFT", 0, -232)
  border:SetScript("OnEnter", function(self)
    showTooltip(self, "Border", "The window's edge texture. Shows anything another add-on has registered with LibSharedMedia, such as an ElvUI or Ellesmere UI skin pack."
      .. (EUI_Skin and " Ignored while \"Match EllesmereUI skin\" is on — that skin draws the border instead." or ""))
  end)
  border:SetScript("OnLeave", function() GameTooltip:Hide() end)
  local borderLabel = v:CreateFontString(nil, "OVERLAY")
  borderLabel:SetFontObject(GameFontHighlightSmall)
  borderLabel:SetPoint("BOTTOMLEFT", border, "TOPLEFT", 2, 3)
  borderLabel:SetText("Border")
  add({ Sync = function() border.fs:SetText(o().borderStyle) end })

  local fontBtn = dropdown(v, 210, 10, function() return mediaList("font", "Default") end, function(name)
    o().fontStyle = name
    applySettings()
    v.Sync()
  end)
  fontBtn:SetPoint("TOPLEFT", 0, -280)
  fontBtn:SetScript("OnEnter", function(self)
    showTooltip(self, "Font", "The typeface used throughout the window, from any add-on's registered font pack.")
  end)
  fontBtn:SetScript("OnLeave", function() GameTooltip:Hide() end)
  local fontLabel = v:CreateFontString(nil, "OVERLAY")
  fontLabel:SetFontObject(GameFontHighlightSmall)
  fontLabel:SetPoint("BOTTOMLEFT", fontBtn, "TOPLEFT", 2, 3)
  fontLabel:SetText("Font")
  add({ Sync = function() fontBtn.fs:SetText(o().fontStyle) end })

  -- Right column: behaviour
  local x = 250
  heading("Behaviour", x, 0)
  local checks = {
    { "Lock window position", "Stops the window being dragged by accident.", "locked" },
    { "Keep window on screen", "Stops the window being dragged partly off the screen.", "clamp" },
    { "Close with Escape", "Pressing Escape closes the window, like the game's own windows.", "escape" },
    { "Only this realm's characters", "Hides characters from other realms in the list and gold total.", "thisRealmOnly" },
  }
  for i, c in ipairs(checks) do
    add(checkbox(v, c[1], c[2], function() return o()[c[3]] end, function(val) o()[c[3]] = val end))
      :SetPoint("TOPLEFT", x, -20 - (i - 1) * 26)
  end
  add(checkbox(v, "Show minimap button", "The minimap button opens this window. /rb and the add-on compartment always work.",
    function() return API.IsMinimapButtonShown() end, function(val) API.SetMinimapButtonShown(val) end))
    :SetPoint("TOPLEFT", x, -20 - #checks * 26)

  if EllesmereUI and EllesmereUI.RegisterSkin then
    add(checkbox(v, "Match EllesmereUI skin", "Use EllesmereUI's own artwork, borders and fonts instead of this window's built-in look. Takes effect after /reload.",
      function() return o().useEUISkin end, function(val) o().useEUISkin = val end))
      :SetPoint("TOPLEFT", x, -20 - (#checks + 1) * 26)
  end

  local reset = button(v, "Reset position", 110, function()
    frame:ClearAllPoints()
    frame:SetPoint("CENTER")
    cfg.point = nil
  end)
  reset:SetPoint("TOPLEFT", x, -186)
  local defaults = button(v, "Restore defaults", 110, function()
    for k, val in pairs(DEFAULTS) do cfg.settings[k] = val end
    API.SetMinimapButtonShown(true)
    applySettings()
    Panel.Refresh()
  end)
  defaults:SetPoint("LEFT", reset, "RIGHT", 6, 0)

  local hint = v:CreateFontString(nil, "OVERLAY")
  hint:SetFontObject(GameFontDisableSmall)
  hint:SetPoint("BOTTOMLEFT", 0, 6)
  hint:SetPoint("RIGHT", v, "RIGHT", 0, 0)
  hint:SetJustifyH("LEFT")
  hint:SetText("/rb opens this window  ·  /rb scan rescans  ·  /rb status  ·  /rb minimap  ·  click a character to close settings")

  local close = button(v, "Close settings", 110, function()
    cfg.tab = lastTab
    Panel.Refresh()
  end)
  close:SetPoint("TOPRIGHT", v, "TOPRIGHT", 0, 4)

  function v.Sync() for _, c in ipairs(controls) do c.Sync() end end
  return v
end

local function savePosition()
  local point, _, relPoint, x, y = frame:GetPoint(1)
  cfg.point = { point, relPoint, x, y }
end

local function build()
  EUI_Skin = nil
  if cfg.settings.useEUISkin and EllesmereUI and EllesmereUI.RegisterSkin then
    -- Fires synchronously here: we're a load-on-demand add-on built well
    -- after PLAYER_LOGIN, which is the "register later" case the API docs
    -- call out, so EUI_Skin is already set by the time build() continues.
    EllesmereUI.RegisterSkin("RosterBook", function(skin) EUI_Skin = skin end)
  end

  frame = tryCreate("Frame", "RosterbookPanel", UIParent, "BackdropTemplate")
  frame:SetSize(WIDTH, HEIGHT)
  frame:SetToplevel(true)
  frame:SetMovable(true)
  frame:EnableMouse(true)
  frame:RegisterForDrag("LeftButton")
  frame:SetScript("OnDragStart", function(self)
    if not cfg.settings.locked then self:StartMoving() end
  end)
  frame:SetScript("OnDragStop", function(self) self:StopMovingOrSizing(); savePosition() end)
  setBackdrop(frame)
  if EUI_Skin then EUI_Skin.Shell(frame) end
  if cfg.point then
    frame:SetPoint(cfg.point[1], UIParent, cfg.point[2], cfg.point[3], cfg.point[4])
  else
    frame:SetPoint("CENTER")
  end

  local title = frame:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
  title:SetPoint("TOPLEFT", 16, -14)
  title:SetText(ACCENT_HEX .. "RosterBook|r")

  -- A plain "x", not UIPanelCloseButton, so a button-reskinning add-on has
  -- no stock close-button texture of ours to recolour.
  local close = CreateFrame("Button", nil, frame)
  close:SetSize(20, 20)
  close:SetPoint("TOPRIGHT", -6, -6)
  local closeFs = close:CreateFontString(nil, "OVERLAY")
  closeFs:SetFontObject(Fonts.header)
  closeFs:SetPoint("CENTER")
  closeFs:SetText(WHITE .. "x|r")
  close.hl = close:CreateTexture(nil, "HIGHLIGHT")
  close.hl:SetAllPoints()
  close.hl:SetColorTexture(1, 1, 1, 0.1)
  close:SetScript("OnClick", function() frame:Hide() end)
  if EUI_Skin then EUI_Skin.CloseButton(close); closeFs:Hide() end

  settingsButton = CreateFrame("Button", nil, frame)
  settingsButton:SetSize(18, 18)
  settingsButton:SetPoint("RIGHT", close, "LEFT", -2, 0)
  settingsButton:SetNormalTexture("Interface\\Icons\\INV_Misc_Gear_01")
  settingsButton:SetHighlightTexture("Interface\\Buttons\\ButtonHilight-Square", "ADD")
  settingsButton:SetScript("OnClick", function()
    if cfg.tab == "settings" then cfg.tab = lastTab else lastTab = cfg.tab; cfg.tab = "settings" end
    Panel.Refresh()
  end)
  settingsButton:SetScript("OnEnter", function(self) showTooltip(self, "Settings", "Opacity, text size, scale, lock and more.") end)
  settingsButton:SetScript("OnLeave", function() GameTooltip:Hide() end)
  -- Icon-only button (the gear), not a labeled one S.Button expects — skip
  -- skinning it so the icon isn't faded out with nothing put in its place.

  -- Left: totals summary first, then sort/filter, then the character list —
  -- same top-to-bottom order as the desktop app's sidebar.
  totalText = frame:CreateFontString(nil, "OVERLAY")
  totalText:SetFontObject(Fonts.row)
  totalText:SetPoint("TOPLEFT", 18, -40)
  totalText:SetJustifyH("LEFT")

  sortButton = button(frame, "Sort", 120, function()
    local _, i = sortSpec()
    cfg.sort = SORTS[i % #SORTS + 1].key
    Panel.Refresh()
  end)
  sortButton:SetPoint("TOPLEFT", totalText, "BOTTOMLEFT", -4, -10)

  local rescan = button(frame, "Rescan", 80, function() API.Rescan(); Panel.Refresh(true) end)
  rescan:SetPoint("LEFT", sortButton, "RIGHT", 6, 0)

  charList = rowList(frame, LIST_W - 24)
  charList.scroll:SetPoint("TOPLEFT", sortButton, "BOTTOMLEFT", 4, -22)
  charList.scroll:SetPoint("BOTTOMLEFT", 14, 12)
  charList.scroll:SetWidth(LIST_W - 24)
  if EUI_Skin and charList.scroll.ScrollBar then EUI_Skin.ScrollBar(charList.scroll.ScrollBar) end

  -- Column headers for the RXP/Gold columns each row's bottom line shows,
  -- anchored at the same fixed x offsets as the row columns themselves.
  local rxpHeader = frame:CreateFontString(nil, "OVERLAY")
  rxpHeader:SetFontObject(Fonts.row)
  rxpHeader:SetJustifyH("RIGHT")
  rxpHeader:SetTextColor(0.6, 0.64, 0.72)
  rxpHeader:SetText("RXP")
  rxpHeader:SetPoint("BOTTOMRIGHT", charList.scroll, "TOPRIGHT", RXP_COL_X, 4)

  local goldHeader = frame:CreateFontString(nil, "OVERLAY")
  goldHeader:SetFontObject(Fonts.row)
  goldHeader:SetJustifyH("RIGHT")
  goldHeader:SetTextColor(0.6, 0.64, 0.72)
  goldHeader:SetText("Gold")
  goldHeader:SetPoint("BOTTOMRIGHT", charList.scroll, "TOPRIGHT", GOLD_COL_X, 4)

  local divider = frame:CreateTexture(nil, "ARTWORK")
  divider:SetColorTexture(THEME.border[1], THEME.border[2], THEME.border[3], 1)
  divider:SetPoint("TOPLEFT", LIST_W + 14, -40)
  divider:SetPoint("BOTTOMLEFT", LIST_W + 14, 12)
  divider:SetWidth(1)

  -- Right: tabs, header, content
  tabButtons = {}
  local prev
  for _, t in ipairs(TAB_ORDER) do
    -- Plain text + underline, not UIPanelButtonTemplate, so a button-reskinning
    -- add-on has no stock button texture of ours to recolour.
    local b = CreateFrame("Button", nil, frame)
    b:SetHeight(22)
    b.fs = b:CreateFontString(nil, "OVERLAY")
    b.fs:SetFontObject(Fonts.row)
    b.fs:SetPoint("CENTER")
    b.fs:SetText(t.label)
    b._text = t.label
    b.label = t.label
    b:SetWidth(math.max(48, b.fs:GetStringWidth() + 16))
    b.key = t.key
    b.selected = b:CreateTexture(nil, "OVERLAY")
    b.selected:SetColorTexture(THEME.accent[1], THEME.accent[2], THEME.accent[3], 0.9)
    b.selected:SetPoint("BOTTOMLEFT", 6, 2)
    b.selected:SetPoint("BOTTOMRIGHT", -6, 2)
    b.selected:SetHeight(2)
    b:SetScript("OnClick", function()
      cfg.tab = t.key
      lastTab = t.key
      Panel.Refresh()
    end)
    if prev then b:SetPoint("LEFT", prev, "RIGHT", 2, 0) else b:SetPoint("TOPLEFT", LIST_W + 26, TABS_TOP) end
    prev = b
    if EUI_Skin then EUI_Skin.Tab(b) end
    tabButtons[#tabButtons + 1] = b
  end

  -- Name + gold, then subtitle + last-seen, then the level/XP bar — same
  -- order as the desktop app, all above the tab row.
  detailTitle = frame:CreateFontString(nil, "OVERLAY")
  detailTitle:SetFontObject(Fonts.title)
  detailTitle:SetPoint("TOPLEFT", LIST_W + 30, TITLE_TOP)
  detailGold = frame:CreateFontString(nil, "OVERLAY")
  detailGold:SetFontObject(Fonts.row)
  detailGold:SetJustifyH("RIGHT")
  detailGold:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -32, TITLE_TOP - 6)
  detailSub = frame:CreateFontString(nil, "OVERLAY")
  detailSub:SetFontObject(Fonts.row)
  detailSub:SetPoint("TOPLEFT", detailTitle, "BOTTOMLEFT", 0, -3)
  detailSub:SetPoint("RIGHT", detailGold, "LEFT", -10, 0)
  detailSub:SetJustifyH("LEFT")
  detailLastSeen = frame:CreateFontString(nil, "OVERLAY")
  detailLastSeen:SetFontObject(Fonts.row)
  detailLastSeen:SetJustifyH("RIGHT")
  detailLastSeen:SetTextColor(0.6, 0.64, 0.72)
  detailLastSeen:SetPoint("TOPRIGHT", detailGold, "BOTTOMRIGHT", 0, -3)

  progressBar = CreateFrame("Frame", nil, frame)
  progressBar:SetPoint("TOPLEFT", detailSub, "BOTTOMLEFT", 0, -10)
  progressBar:SetPoint("RIGHT", frame, "RIGHT", -32, 0)
  progressBar:SetHeight(32)
  progressBar.bar = CreateFrame("StatusBar", nil, progressBar)
  progressBar.bar:SetStatusBarTexture("Interface\\TargetingFrame\\UI-StatusBar")
  progressBar.bar:SetHeight(10)
  progressBar.bar:SetPoint("TOPLEFT", 0, 0)
  progressBar.bar:SetPoint("RIGHT", 0, 0)
  progressBar.bar.bg = progressBar.bar:CreateTexture(nil, "BACKGROUND")
  progressBar.bar.bg:SetAllPoints()
  progressBar.bar.bg:SetColorTexture(0, 0, 0, 0.5)
  progressBar.left = progressBar:CreateFontString(nil, "OVERLAY")
  progressBar.left:SetFontObject(Fonts.row)
  progressBar.left:SetJustifyH("LEFT")
  progressBar.left:SetPoint("TOPLEFT", progressBar.bar, "BOTTOMLEFT", 0, -3)
  progressBar.right = progressBar:CreateFontString(nil, "OVERLAY")
  progressBar.right:SetFontObject(Fonts.row)
  progressBar.right:SetJustifyH("RIGHT")
  progressBar.right:SetTextColor(0.6, 0.64, 0.72)
  progressBar.right:SetPoint("TOPRIGHT", progressBar.bar, "BOTTOMRIGHT", 0, -3)

  -- A standing search bar next to the title, not a tab: typing in it
  -- overrides whatever tab is open with cross-character results, like the
  -- desktop app's search field.
  findBox = tryCreate("EditBox", nil, frame, "InputBoxTemplate")
  findBox:SetSize(220, 26)
  findBox:SetAutoFocus(false)
  -- Anchored off the title's own right edge (not fixed numbers), so it sits
  -- right next to "RosterBook" and shares its vertical centre.
  findBox:SetPoint("LEFT", title, "RIGHT", 18, 0)
  if findBox.SetTextInsets then findBox:SetTextInsets(18, 4, 0, 0) end
  local findIcon = frame:CreateTexture(nil, "OVERLAY")
  findIcon:SetSize(14, 14)
  findIcon:SetTexture("Interface\\Common\\UI-Searchbox-Icon")
  findIcon:SetPoint("LEFT", findBox, "LEFT", 4, 0)
  findHint = frame:CreateFontString(nil, "OVERLAY")
  findHint:SetFontObject(Fonts.row)
  findHint:SetTextColor(0.5, 0.54, 0.6)
  findHint:SetPoint("LEFT", findIcon, "RIGHT", 4, 0)
  findHint:SetText("Find an item on any character")
  local function syncFindHint() if findBox:GetText() == "" then findHint:Show() else findHint:Hide() end end
  findBox:SetScript("OnTextChanged", function() syncFindHint(); Panel.Refresh() end)
  findBox:SetScript("OnEscapePressed", function(self) self:SetText(""); self:ClearFocus() end)
  findBox:SetScript("OnEnterPressed", function(self) self:ClearFocus() end)
  syncFindHint()
  if EUI_Skin then EUI_Skin.EditBox(findBox, { padInput = true }) end

  overviewGrid = tileGrid(frame)
  overviewGrid.frame:SetPoint("TOPLEFT", frame, "TOPLEFT", LIST_W + 28, CONTENT_TOP)
  overviewGrid.frame:Hide()

  detail = rowList(frame, WIDTH - LIST_W - 60)
  detailGrid = sectionList(frame, WIDTH - LIST_W - 60)
  detailGrid.scroll:Hide()
  if EUI_Skin then
    if detail.scroll.ScrollBar then EUI_Skin.ScrollBar(detail.scroll.ScrollBar) end
    if detailGrid.scroll.ScrollBar then EUI_Skin.ScrollBar(detailGrid.scroll.ScrollBar) end
  end
  settingsView = buildSettingsView()

  frame:SetScript("OnShow", function() Panel.Refresh() end)
  frame:Hide()
  applySettings()
end

StaticPopupDialogs = StaticPopupDialogs or {}
StaticPopupDialogs.ROSTERBOOK_FORGET = {
  text = "Remove %s from RosterBook?\n\nLogging in on that character records it again.",
  button1 = YES or "Yes",
  button2 = NO or "No",
  OnAccept = function(_, guid)
    if API.ForgetCharacter(guid) then
      selected = API.GetCurrentGUID()
      Panel.Refresh()
    end
  end,
  timeout = 0, whileDead = true, hideOnEscape = true, preferredIndex = 3,
}

function Panel.ShowSettings()
  Panel.Show("settings")
end

function Panel.Toggle()
  if not cfg then loadSettings() end
  if not frame then build() end
  if frame:IsShown() then frame:Hide() else frame:Show() end
end

function Panel.Show(tab)
  if not cfg then loadSettings() end
  if not frame then build() end
  if tab then cfg.tab = tab end
  frame:Show()
  Panel.Refresh()
end

-- A load-on-demand add-on's saved variables arrive after its files have run,
-- just before ADDON_LOADED (which fires inside the LoadAddOn call).
local loader = CreateFrame("Frame")
loader:RegisterEvent("ADDON_LOADED")
loader:SetScript("OnEvent", function(self, _, name)
  if name ~= "Rosterbook_UI" then return end
  self:UnregisterEvent("ADDON_LOADED")
  loadSettings()
  selected = cfg.selected
  applyFonts(cfg.settings.fontSize)
end)
API.RegisterCallback(function() Panel.Refresh(true) end)
