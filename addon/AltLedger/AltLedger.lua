--[[
  Alt Ledger — exporter add-on

  Records what the Alt Ledger companion app shows (gold, XP, rested XP,
  location, bags, bank, equipped gear, professions, reputations, mail) into
  the AltLedgerDB saved variable. WoW writes it to disk when you log out or
  /reload; the companion app reads it from there.

  Everything is stored as plain, readable values — no bit packing — so the
  file format is easy to read from any language. Bump SCHEMA if it changes.

  Free and open source (MIT). This add-on never asks for anything in game.
]]

local ADDON = ...
local SCHEMA = 1

-- ---------------------------------------------------------------------------
-- Compatibility shims (Forever is a modern client with classic-style systems)
-- ---------------------------------------------------------------------------
local C_Container = C_Container
local GetContainerNumSlots = C_Container and C_Container.GetContainerNumSlots or GetContainerNumSlots
local GetContainerItemInfo = C_Container and C_Container.GetContainerItemInfo
local GetContainerItemLink = C_Container and C_Container.GetContainerItemLink or GetContainerItemLink
local GetContainerNumFreeSlots = C_Container and C_Container.GetContainerNumFreeSlots or GetContainerNumFreeSlots
local ContainerIDToInventoryID = C_Container and C_Container.ContainerIDToInventoryID or ContainerIDToInventoryID

local BagIndex = Enum and Enum.BagIndex or {}
local BACKPACK = BagIndex.Backpack or 0
local NUM_BAGS = NUM_BAG_SLOTS or 4
local REAGENT_BAG = BagIndex.ReagentBag or 5              -- profession bag slot in Forever
local BANK_MAIN = BagIndex.Bank or -1                     -- old-style main bank slots
local BANK_FIRST = BagIndex.CharacterBankTab_1 or 6       -- bank tabs / bank bags
local BANK_LAST = BagIndex.CharacterBankTab_6 or 12
local WARBAND_FIRST = BagIndex.AccountBankTab_1            -- Retail only: account-wide bank
local WARBAND_LAST = BagIndex.AccountBankTab_5

local IS_RETAIL = WOW_PROJECT_ID and WOW_PROJECT_MAINLINE and WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
local GetItemInfoCompat = (C_Item and C_Item.GetItemInfo) or GetItemInfo

-- Retail names its bank tabs; returns { [bagId] = "Tab name" }.
local function bankTabNames(bankType)
  local names = {}
  if C_Bank and C_Bank.FetchPurchasedBankTabData and Enum.BankType and bankType then
    local ok, tabs = pcall(C_Bank.FetchPurchasedBankTabData, bankType)
    if ok and tabs then
      for _, t in ipairs(tabs) do
        if t.ID and t.name and t.name ~= "" then names[t.ID] = t.name end
      end
    end
  end
  return names
end

-- Item category (class / subclass / equip slot) lets the companion app draw
-- its own generic icon for each item instead of using Blizzard's artwork.
local GetItemInfoInstant = (C_Item and C_Item.GetItemInfoInstant) or GetItemInfoInstant
local function itemCategory(itemId)
  if not (GetItemInfoInstant and itemId) then return nil end
  local ok, _, _, _, equipLoc, _, classId, subclassId = pcall(GetItemInfoInstant, itemId)
  if not ok then return nil end
  return classId, subclassId, (equipLoc and equipLoc ~= "") and equipLoc or nil
end

local function itemFromLink(link)
  if not link then return nil end
  local id = tonumber(link:match("item:(%d+)"))
  local name = link:match("|h%[(.-)%]|h")
  return id, name
end

-- Full display name. Forever adds an optional secondary (last) name.
local function playerName()
  local first = UnitName("player")
  local second
  if C_PlayerInfo and C_PlayerInfo.GetSecondaryName then
    local ok, v = pcall(C_PlayerInfo.GetSecondaryName)
    if ok and type(v) == "string" and v ~= "" then second = v end
  end
  if second and not first:find(second, 1, true) then
    return first .. " " .. second, first, second
  end
  local f, s = first:match("^(%S+)%s+(.+)$")
  return first, f or first, s
end

-- ---------------------------------------------------------------------------
-- Database
-- ---------------------------------------------------------------------------
local db, me

local function ensureDB()
  AltLedgerDB = AltLedgerDB or {}
  AltLedgerDB.schema = SCHEMA
  AltLedgerDB.game = IS_RETAIL and "retail" or "classic"
  AltLedgerDB.build = select(1, GetBuildInfo())
  AltLedgerDB.characters = AltLedgerDB.characters or {}
  db = AltLedgerDB

  local guid = UnitGUID("player")
  db.characters[guid] = db.characters[guid] or {}
  me = db.characters[guid]
  me.guid = guid
end

-- ---------------------------------------------------------------------------
-- Scanners
-- ---------------------------------------------------------------------------
local function scanBasics()
  local full, first, second = playerName()
  local className, classToken, classId = UnitClass("player")
  local raceName, raceToken, raceId = UnitRace("player")
  local powerType, powerToken = UnitPowerType("player")

  me.name = full
  me.firstName = first
  me.secondName = second
  me.realm = GetRealmName()
  me.level = UnitLevel("player")
  me.class = className
  me.classToken = classToken
  me.classId = classId
  me.race = raceName
  me.raceToken = raceToken
  me.raceId = raceId
  me.sex = UnitSex("player")            -- 2 male, 3 female
  me.faction = UnitFactionGroup("player")
  me.healthMax = UnitHealthMax("player")
  me.powerToken = powerToken
  me.powerMax = UnitPowerMax("player", powerType)
  me.lastUpdate = time()
end

local function scanMoney()
  me.money = GetMoney()
  me.lastUpdate = time()
end

local function scanXP()
  me.level = UnitLevel("player")
  me.xp = UnitXP("player")
  me.xpMax = UnitXPMax("player")
  me.restXP = GetXPExhaustion() or 0
  me.resting = IsResting() and true or false
  me.xpDisabled = (IsXPUserDisabled and IsXPUserDisabled()) and true or false
  me.maxLevel = (IsPlayerAtEffectiveMaxLevel and IsPlayerAtEffectiveMaxLevel()) and true or false
end

local function scanLocation()
  me.zone = GetRealZoneText()
  me.subZone = GetSubZoneText()
  me.bindLocation = GetBindLocation()
  me.resting = IsResting() and true or false
end

local function readBag(bagId)
  local size = GetContainerNumSlots(bagId) or 0
  if size == 0 then return nil end

  local bag = { size = size, free = GetContainerNumFreeSlots(bagId) or 0, items = {} }

  if bagId ~= BACKPACK and bagId ~= BANK_MAIN and ContainerIDToInventoryID then
    local ok, invId = pcall(ContainerIDToInventoryID, bagId)
    if ok and invId then
      local _, name = itemFromLink(GetInventoryItemLink("player", invId))
      bag.name = name
      bag.itemId = GetInventoryItemID("player", invId)
    end
  end

  for slot = 1, size do
    local info = GetContainerItemInfo and GetContainerItemInfo(bagId, slot)
    local link = (info and info.hyperlink) or GetContainerItemLink(bagId, slot)
    if link then
      local id, name = itemFromLink(link)
      id = (info and info.itemID) or id
      local classId, subclassId, equipLoc = itemCategory(id)
      bag.items[#bag.items + 1] = {
        slot = slot,
        id = id,
        count = (info and info.stackCount) or 1,
        quality = info and info.quality,
        icon = info and info.iconFileID,
        name = name,
        classId = classId,
        subclassId = subclassId,
        equipLoc = equipLoc,
      }
    end
  end
  return bag
end

local function scanBags()
  if (GetContainerNumSlots(BACKPACK) or 0) == 0 then return end  -- bags not loaded yet
  local bags = {}
  for bagId = BACKPACK, NUM_BAGS do
    local b = readBag(bagId)
    if b then b.id = bagId; bags[#bags + 1] = b end
  end
  if REAGENT_BAG and REAGENT_BAG > NUM_BAGS and REAGENT_BAG < BANK_FIRST then
    local b = readBag(REAGENT_BAG)
    if b then b.id = REAGENT_BAG; b.profession = true; bags[#bags + 1] = b end
  end
  me.bags = bags
  me.lastUpdate = time()
end

local bankOpen = false
local function scanBank()
  if not bankOpen then return end
  local bank = {}
  local tabNames = bankTabNames(Enum.BankType and Enum.BankType.Character)
  local b = readBag(BANK_MAIN)
  if b then b.id = BANK_MAIN; b.name = b.name or "Bank"; bank[#bank + 1] = b end
  for bagId = BANK_FIRST, BANK_LAST do
    b = readBag(bagId)
    if b then
      b.id = bagId
      if tabNames[bagId] then
        b.name = tabNames[bagId]
      elseif not b.name or b.name:find("%(DNT%)") then
        b.name = "Bank tab " .. (bagId - BANK_FIRST + 1)
      end
      bank[#bank + 1] = b
    end
  end
  -- Never overwrite a saved bank with an empty read (e.g. a frame closing mid-scan).
  if #bank > 0 then
    me.bank = bank
    me.bankScanned = time()
  end

  -- Retail's Warband bank is shared by every character on the account.
  if WARBAND_FIRST and WARBAND_LAST then
    local names = bankTabNames(Enum.BankType and Enum.BankType.Account)
    local tabs = {}
    for bagId = WARBAND_FIRST, WARBAND_LAST do
      local w = readBag(bagId)
      if w then
        w.id = bagId
        w.name = names[bagId] or ("Warband tab " .. (bagId - WARBAND_FIRST + 1))
        tabs[#tabs + 1] = w
      end
    end
    if #tabs > 0 then
      db.warband = { tabs = tabs, scanned = time(), money = (C_Bank and C_Bank.FetchDepositedMoney
        and Enum.BankType and C_Bank.FetchDepositedMoney(Enum.BankType.Account)) or nil }
    end
  end
end

local function scanEquipment()
  local gear = {}
  local total, count = 0, 0
  for slot = 1, 19 do
    local link = GetInventoryItemLink("player", slot)
    if link then
      local id, name = itemFromLink(link)
      id = GetInventoryItemID("player", slot) or id
      local classId, subclassId, equipLoc = itemCategory(id)
      gear[#gear + 1] = {
        slot = slot,
        id = id,
        name = name,
        classId = classId,
        subclassId = subclassId,
        equipLoc = equipLoc,
        quality = GetInventoryItemQuality("player", slot),
        icon = GetInventoryItemTexture("player", slot),
      }
      if slot ~= 4 and slot ~= 19 then   -- shirt and tabard don't count
        local ilvl = (C_Item and C_Item.GetDetailedItemLevelInfo and C_Item.GetDetailedItemLevelInfo(link))
          or (GetItemInfoCompat and select(4, GetItemInfoCompat(link)))
        if ilvl then total = total + ilvl; count = count + 1 end
      end
    end
  end
  me.equipment = gear
  if GetAverageItemLevel then
    me.itemLevel = select(2, GetAverageItemLevel())
  elseif count > 0 then
    me.itemLevel = total / 16
  end
end

-- Walks a header/row list, temporarily expanding collapsed headers so nothing
-- is missed, then puts the player's UI back the way it was.
local function withExpandedHeaders(getNum, getInfo, expand, collapse, visit)
  local collapsed = {}
  for i = getNum(), 1, -1 do
    local name, isHeader, isExpanded = getInfo(i)
    if isHeader and not isExpanded then
      collapsed[name] = true
      expand(i)
    end
  end
  for i = 1, getNum() do visit(i) end
  for i = getNum(), 1, -1 do
    local name, isHeader = getInfo(i)
    if isHeader and collapsed[name] then collapse(i) end
  end
end

-- Retail: professions come from GetProfessions(), not the skill list.
local function scanProfessionsRetail()
  local profs = {}
  local p1, p2, arch, fish, cook = GetProfessions()
  local function add(index, kind)
    if not index then return end
    local name, _, rank, maxRank, _, _, _, _, _, _, lineName = GetProfessionInfo(index)
    if name then
      profs[#profs + 1] = { name = name, rank = rank, max = maxRank, kind = kind,
                            expansion = (lineName and lineName ~= name) and lineName or nil }
    end
  end
  add(p1, "primary"); add(p2, "primary")
  add(cook, "secondary"); add(fish, "secondary"); add(arch, "secondary")
  me.professions = profs
end

local function scanSkills()
  if not GetNumSkillLines then
    if GetProfessions then scanProfessionsRetail() end
    return
  end
  local profs, header = {}, nil
  local wanted = { [TRADE_SKILLS or "Professions"] = "primary", [SECONDARY_SKILLS or "Secondary Skills"] = "secondary" }
  withExpandedHeaders(
    GetNumSkillLines,
    function(i) local n, h, e = GetSkillLineInfo(i); return n, h, e end,
    ExpandSkillHeader, CollapseSkillHeader,
    function(i)
      local name, isHeader, _, rank, _, _, maxRank = GetSkillLineInfo(i)
      if isHeader then
        header = wanted[name]
      elseif header then
        profs[#profs + 1] = { name = name, rank = rank, max = maxRank, kind = header }
      end
    end)
  me.professions = profs
end

local function scanReputations()
  local getNum = GetNumFactions or (C_Reputation and C_Reputation.GetNumFactions)
  if not getNum then return end

  local function info(i)
    if GetFactionInfo then
      local name, _, standing, barMin, barMax, value, _, _, isHeader, isCollapsed, hasRep, _, _, factionId = GetFactionInfo(i)
      return { name = name, standing = standing, min = barMin, max = barMax, value = value,
               isHeader = isHeader, isCollapsed = isCollapsed, hasRep = hasRep, id = factionId }
    end
    local d = C_Reputation.GetFactionDataByIndex(i)
    if not d then return {} end
    return { name = d.name, standing = d.reaction, min = d.currentReactionThreshold, max = d.nextReactionThreshold,
             value = d.currentStanding, isHeader = d.isHeader, isCollapsed = d.isCollapsed,
             hasRep = d.isHeaderWithRep, id = d.factionID }
  end
  local expand = ExpandFactionHeader or C_Reputation.ExpandFactionHeader
  local collapse = CollapseFactionHeader or C_Reputation.CollapseFactionHeader

  local reps, group = {}, nil
  withExpandedHeaders(getNum,
    function(i) local f = info(i); return f.name, f.isHeader, not f.isCollapsed end,
    expand, collapse,
    function(i)
      local f = info(i)
      if f.isHeader then group = f.name end
      if f.id and f.id > 0 and (not f.isHeader or f.hasRep) then
        local rep = {
          id = f.id, name = f.name, group = group,
          kind = "standing",
          standing = f.standing,          -- 1 Hated … 4 Neutral … 8 Exalted
          value = f.value,                -- total earned (can be negative)
          min = f.min, max = f.max,       -- thresholds of the current standing
        }
        -- Retail-only reputation types. "label" replaces Hated…Exalted for these.
        if IS_RETAIL and C_Reputation.IsMajorFaction and C_Reputation.IsMajorFaction(f.id) then
          local m = C_MajorFactions and C_MajorFactions.GetMajorFactionData(f.id)
          if m then
            rep.kind, rep.label = "renown", "Renown " .. (m.renownLevel or 0)
            rep.value, rep.min, rep.max = m.renownReputationEarned or 0, 0, m.renownLevelThreshold or 0
          end
        elseif IS_RETAIL and C_GossipInfo and C_GossipInfo.GetFriendshipReputation then
          local fr = C_GossipInfo.GetFriendshipReputation(f.id)
          if fr and fr.friendshipFactionID and fr.friendshipFactionID > 0 then
            rep.kind, rep.label = "friendship", fr.reaction
            rep.value, rep.min = fr.standing or 0, fr.reactionThreshold or 0
            rep.max = fr.nextThreshold or rep.value
          end
        end
        reps[#reps + 1] = rep
      end
    end)
  me.reputations = reps
end

local function scanMail()
  if not GetInboxNumItems then return end
  local num = GetInboxNumItems()
  local soonest
  for i = 1, num do
    local daysLeft = select(7, GetInboxHeaderInfo(i))
    if daysLeft and (not soonest or daysLeft < soonest) then soonest = daysLeft end
  end
  me.mail = { count = num, soonestExpiry = soonest and (time() + math.floor(soonest * 86400)) or nil, scanned = time() }
end

-- Time played arrives via TIME_PLAYED_MSG. We ask once per login and hide the
-- chat line that our own request would otherwise print.
local playedAt
local hidePlayedMessage = false
local function requestPlayed()
  if not RequestTimePlayed then return end
  hidePlayedMessage = true
  RequestTimePlayed()
end
-- Retail's chat code is stricter about add-ons replacing its functions, so we
-- only hide the line on classic-style clients (Forever). On Retail, the
-- "Total time played" line simply shows once at login.
if not IS_RETAIL and ChatFrame_DisplayTimePlayed then
  local original = ChatFrame_DisplayTimePlayed
  ChatFrame_DisplayTimePlayed = function(...)
    if hidePlayedMessage then hidePlayedMessage = false; return end
    return original(...)
  end
end

local function scanAll()
  scanBasics(); scanMoney(); scanXP(); scanLocation()
  scanBags(); scanEquipment(); scanSkills(); scanReputations()
end

-- ---------------------------------------------------------------------------
-- Events
-- ---------------------------------------------------------------------------
local frame = CreateFrame("Frame")
local handlers = {}
local pending = {}

-- Coalesce bursts of events (looting, vendoring) into one scan per second.
local function soon(key, fn)
  if pending[key] then return end
  pending[key] = true
  C_Timer.After(1, function() pending[key] = nil; if me then fn() end end)
end

handlers.PLAYER_LOGIN = function()
  ensureDB()
  scanAll()
  requestPlayed()
end
handlers.PLAYER_ENTERING_WORLD = function() if me then scanLocation(); soon("all", scanAll) end end
handlers.BAG_UPDATE_DELAYED = function() soon("bags", function() scanBags(); scanBank() end) end
handlers.PLAYER_MONEY = function() if me then scanMoney() end end
handlers.PLAYER_XP_UPDATE = function() if me then scanXP() end end
handlers.UPDATE_EXHAUSTION = handlers.PLAYER_XP_UPDATE
handlers.PLAYER_LEVEL_UP = function() soon("all", scanAll) end
handlers.PLAYER_UPDATE_RESTING = function() if me then scanXP(); scanLocation() end end
handlers.ZONE_CHANGED = function() if me then scanLocation() end end
handlers.ZONE_CHANGED_NEW_AREA = handlers.ZONE_CHANGED
handlers.ZONE_CHANGED_INDOORS = handlers.ZONE_CHANGED
handlers.PLAYER_EQUIPMENT_CHANGED = function() soon("gear", scanEquipment) end
handlers.SKILL_LINES_CHANGED = function() soon("skills", scanSkills) end
handlers.UPDATE_FACTION = function() soon("reps", scanReputations) end
handlers.MAIL_INBOX_UPDATE = function() soon("mail", scanMail) end
handlers.BANKFRAME_OPENED = function() bankOpen = true; soon("bank", scanBank) end
handlers.BANKFRAME_CLOSED = function() bankOpen = false end
handlers.PLAYERBANKSLOTS_CHANGED = function() soon("bank", scanBank) end
handlers.TIME_PLAYED_MSG = function(total, thisLevel)
  if not me then return end
  me.played, me.playedThisLevel = total, thisLevel
  playedAt = GetTime()
  -- Only ever hide the reply to our own request, never a later /played.
  C_Timer.After(0, function() hidePlayedMessage = false end)
end
handlers.PLAYER_LOGOUT = function()
  if not me then return end
  scanMoney(); scanXP(); scanLocation()
  if playedAt and me.played then
    local extra = math.floor(GetTime() - playedAt)
    me.played = me.played + extra
    me.playedThisLevel = (me.playedThisLevel or 0) + extra
  end
  me.lastLogout = time()
end

for event in pairs(handlers) do
  pcall(frame.RegisterEvent, frame, event)   -- skip events this client doesn't have
end
frame:SetScript("OnEvent", function(_, event, ...) handlers[event](...) end)

-- ---------------------------------------------------------------------------
-- /altledger
-- ---------------------------------------------------------------------------
SLASH_ALTLEDGER1 = "/altledger"
SLASH_ALTLEDGER2 = "/al"
SlashCmdList.ALTLEDGER = function(msg)
  if not me then return end
  if msg == "scan" then
    scanAll(); scanBank()
    print("|cffd9a441Alt Ledger|r: rescanned. Type /reload to save it for the companion app.")
    return
  end
  local n = 0
  for _ in pairs(db.characters) do n = n + 1 end
  local bank = me.bankScanned and date("%b %d %H:%M", me.bankScanned) or "never — visit a banker once"
  print(("|cffd9a441Alt Ledger|r: tracking %d character%s. This character's bank last saved: %s.")
    :format(n, n == 1 and "" or "s", bank))
  print("Data reaches the companion app when you log out or /reload. |cffaaaaaa/altledger scan|r rescans now.")
end
