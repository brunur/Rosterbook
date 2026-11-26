--[[
  RosterBook Disenchant — tooltip profit for disenchanting

  Watches your own disenchants (spell cast + a bag-snapshot diff right
  after, for both the consumed item and whatever mats landed — this client
  never opens a loot window for Disenchant) and remembers the yield per
  item ID. Once an item's been disenchanted at least once and Auctionator
  has prices for it and all its mats, its tooltip gets a one-line profit.

  Deliberately learns instead of guessing from a hardcoded yield table: the
  numbers vary by game version/content tier and we don't have one source of
  truth for them here, so showing nothing beats showing a wrong number. For
  an item you haven't personally disenchanted yet, it'll predict from other
  items of the same quality/item-level bucket you have disenchanted (WoW's
  own yield tiers are driven by quality + item level, not the specific
  item) — clearly labeled "predicted" rather than mixed in as fact.

  Known risk area: the "which item just got disenchanted" detection is a
  bag-snapshot diff around the spell-success event, not a direct read of
  the spell's target. Moving/selling/mailing another item in the same
  breath as disenchanting can make the diff ambiguous — safely discarded
  rather than risking a bad sample.
]]

local ADDON = ...

-- ---------------------------------------------------------------------------
-- Compatibility shims
-- ---------------------------------------------------------------------------
local C_Item = C_Item
local GetItemInfoInstantCompat = (C_Item and C_Item.GetItemInfoInstant) or GetItemInfoInstant
local GetItemInfoCompat = (C_Item and C_Item.GetItemInfo) or GetItemInfo

local C_Container = C_Container
local function containerNumSlots(bag)
  if C_Container and C_Container.GetContainerNumSlots then return C_Container.GetContainerNumSlots(bag) end
  return GetContainerNumSlots(bag)
end
local function containerItemIdAndCount(bag, slot)
  if C_Container and C_Container.GetContainerItemInfo then
    local info = C_Container.GetContainerItemInfo(bag, slot)
    if not info then return nil, 0 end
    return info.itemID, info.stackCount or 1
  end
  local _, count, _, _, _, _, link = GetContainerItemInfo(bag, slot)
  if not link then return nil, 0 end
  local id = GetItemInfoInstantCompat(link)
  return id, count or 1
end

local C_Spell = C_Spell
local function spellNameById(spellId)
  if C_Spell and C_Spell.GetSpellInfo then
    local ok, info = pcall(C_Spell.GetSpellInfo, spellId)
    if ok and info then return info.name end
    return nil
  end
  local ok, name = pcall(GetSpellInfo, spellId)
  if ok then return name end
  return nil
end

-- Enum.ItemClass.* with numeric fallback, in case this client predates the enum.
local CLASS_WEAPON = (Enum and Enum.ItemClass and Enum.ItemClass.Weapon) or 2
local CLASS_ARMOR = (Enum and Enum.ItemClass and Enum.ItemClass.Armor) or 4

-- ---------------------------------------------------------------------------
-- Saved data:
--   db.yields[itemID]   = { n, mats = { [matItemId] = { count, name, icon } } }
--   db.buckets[bucketKey] = same shape, rolled up across every item seen in
--     that quality/item-level bucket — real yield tiers in WoW are driven by
--     quality + item level, not by the specific item, so this lets an item
--     you've never personally disenchanted borrow from others in its bucket
--     instead of needing a hardcoded (and unverifiable on this client) table.
-- ---------------------------------------------------------------------------
local db

local function matNameAndIcon(itemId)
  local name, _, _, _, _, _, _, _, _, icon = GetItemInfoCompat(itemId)
  return name, icon
end

-- Item level buckets matching where WoW's own disenchant yield tiers break.
local function levelBucket(ilvl)
  if ilvl <= 15 then return "1-15" end
  if ilvl <= 25 then return "16-25" end
  if ilvl <= 35 then return "26-35" end
  if ilvl <= 45 then return "36-45" end
  if ilvl <= 55 then return "46-55" end
  return "56+"
end

local function bucketKey(quality, ilvl)
  if not quality or not ilvl then return nil end
  return quality .. ":" .. levelBucket(ilvl)
end

local function addToEntry(entry, mats)
  entry.n = entry.n + 1
  for _, m in ipairs(mats) do
    local mat = entry.mats[m.id]
    if not mat then
      local name, icon = matNameAndIcon(m.id)
      mat = { count = 0, name = name, icon = icon }
      entry.mats[m.id] = mat
    end
    mat.count = mat.count + m.count
  end
end

local function recordDisenchant(itemId, mats)
  if not itemId or #mats == 0 then return end

  local entry = db.yields[itemId]
  if not entry then
    entry = { n = 0, mats = {} }
    db.yields[itemId] = entry
  end
  addToEntry(entry, mats)

  local _, _, quality, ilvl = GetItemInfoCompat(itemId)
  local key = bucketKey(quality, ilvl)
  if key then
    local bucket = db.buckets[key]
    if not bucket then
      bucket = { n = 0, mats = {} }
      db.buckets[key] = bucket
    end
    addToEntry(bucket, mats)
  end
  if quality then
    local qbucket = db.qualityBuckets[quality]
    if not qbucket then
      qbucket = { n = 0, mats = {} }
      db.qualityBuckets[quality] = qbucket
    end
    addToEntry(qbucket, mats)
  end
end

local function loadSettings()
  RosterbookDisenchantDB = RosterbookDisenchantDB or {}
  db = RosterbookDisenchantDB
  db.yields = db.yields or {}
  db.buckets = db.buckets or {}
  db.qualityBuckets = db.qualityBuckets or {}
  local hadBuckets = next(db.buckets) ~= nil
  local hadQualityBuckets = next(db.qualityBuckets) ~= nil

  -- Backfill from any items learned before this tracking existed. Each
  -- rollup backfills independently, since one can already exist (from an
  -- earlier version of this addon) while the other was just added.
  if not hadBuckets or not hadQualityBuckets then
    for itemId, entry in pairs(db.yields) do
      local _, _, quality, ilvl = GetItemInfoCompat(itemId)
      local key = bucketKey(quality, ilvl)
      if not hadBuckets and key then
        local bucket = db.buckets[key]
        if not bucket then
          bucket = { n = 0, mats = {} }
          db.buckets[key] = bucket
        end
        bucket.n = bucket.n + entry.n
        for matId, mat in pairs(entry.mats) do
          local bm = bucket.mats[matId]
          if not bm then
            bm = { count = 0, name = mat.name, icon = mat.icon }
            bucket.mats[matId] = bm
          end
          bm.count = bm.count + mat.count
        end
      end
      if not hadQualityBuckets and quality then
        local qbucket = db.qualityBuckets[quality]
        if not qbucket then
          qbucket = { n = 0, mats = {} }
          db.qualityBuckets[quality] = qbucket
        end
        qbucket.n = qbucket.n + entry.n
        for matId, mat in pairs(entry.mats) do
          local bm = qbucket.mats[matId]
          if not bm then
            bm = { count = 0, name = mat.name, icon = mat.icon }
            qbucket.mats[matId] = bm
          end
          bm.count = bm.count + mat.count
        end
      end
    end
  end
end

-- ---------------------------------------------------------------------------
-- Watching disenchants: bag snapshot right before the spell succeeds, then
-- diffed against the next settled snapshot. No loot window involved — this
-- client hands disenchant mats straight to bags without one (confirmed
-- live: LOOT_OPENED never fires for Disenchant here), so the yield is read
-- the same way the consumed item is: whatever else changed in the bags.
-- ---------------------------------------------------------------------------
local lastSnapshot = {}
local pending -- { preSnapshot, time } once a Disenchant cast has succeeded
local PENDING_TIMEOUT = 5

local function snapshotBags()
  local counts = {}
  local numBags = _G.NUM_BAG_SLOTS or 4
  for bag = 0, numBags do
    local slots = containerNumSlots(bag) or 0
    for slot = 1, slots do
      local id, count = containerItemIdAndCount(bag, slot)
      if id then counts[id] = (counts[id] or 0) + count end
    end
  end
  return counts
end

-- Picks the single item ID whose count dropped by exactly one between two
-- snapshots. Anything less clean (nothing dropped, more than one item
-- dropped, something dropped by more than 1 — e.g. a stack was also moved
-- elsewhere in the same tick) is treated as ambiguous and discarded rather
-- than risk recording a bad sample.
local function diffDisenchantedItem(before, after)
  local dropped, dropCount = nil, 0
  for id, beforeCount in pairs(before) do
    local afterCount = after[id] or 0
    if afterCount < beforeCount then
      dropCount = dropCount + 1
      dropped = id
      if beforeCount - afterCount ~= 1 then return nil end
    end
  end
  if dropCount ~= 1 then return nil end
  return dropped
end

-- Every item (other than `excludeId`) whose count went up between the two
-- snapshots, as the disenchant's yield.
local function diffYieldMats(before, after, excludeId)
  local mats = {}
  for id, afterCount in pairs(after) do
    if id ~= excludeId then
      local gained = afterCount - (before[id] or 0)
      if gained > 0 then mats[#mats + 1] = { id = id, count = gained } end
    end
  end
  return mats
end

-- Temporary tracing while the detection logic gets verified live — toggle
-- with /rbdedebug, off by default.
local DEBUG = false
local function trace(...)
  if DEBUG then print("|cff9c8fe6[RBDE]|r", ...) end
end

local function onEvent(self, event, ...)
  if event == "ADDON_LOADED" then
    local name = ...
    if name == ADDON then loadSettings(); self:UnregisterEvent("ADDON_LOADED") end
    return
  end

  if event == "UNIT_SPELLCAST_SUCCEEDED" then
    local unit, _, spellId = ...
    if unit ~= "player" then return end
    local name = spellNameById(spellId)
    if name ~= "Disenchant" then return end
    pending = { preSnapshot = lastSnapshot, time = GetTime() }
    trace("disenchant cast, armed")
    return
  end

  if event == "BAG_UPDATE_DELAYED" then
    if pending then
      if GetTime() - pending.time > PENDING_TIMEOUT then
        trace("pending timed out")
      else
        local before = pending.preSnapshot
        local after = snapshotBags()
        local itemId = diffDisenchantedItem(before, after)
        if itemId then
          local mats = diffYieldMats(before, after, itemId)
          recordDisenchant(itemId, mats)
          trace("recorded item", itemId, "with", #mats, "mat type(s)")
        else
          trace("ambiguous bag diff, discarded")
        end
      end
      pending = nil
    end
    lastSnapshot = snapshotBags()
    return
  end
end

-- Fires on every bag change (not just disenchants), so it's pcall-wrapped
-- for the same reason the tooltip handler is: an unprotected error here
-- would fire the "too many errors" banner on ordinary bag activity.
local watcher = CreateFrame("Frame")
watcher:RegisterEvent("ADDON_LOADED")
watcher:RegisterEvent("UNIT_SPELLCAST_SUCCEEDED")
watcher:RegisterEvent("BAG_UPDATE_DELAYED")
watcher:SetScript("OnEvent", function(self, event, ...)
  local ok, err = pcall(onEvent, self, event, ...)
  if not ok then print("|cffff0000[RBDE ERROR]|r", err) end
end)

-- GetCoinTextureString is nil on this client (confirmed live: "attempt to
-- call a nil value" where it was used) — plain gold/silver/copper text.
local function coinText(copper)
  local sign = copper < 0 and "-" or ""
  copper = math.floor(math.abs(copper) + 0.5)
  local gold = math.floor(copper / 10000)
  local silver = math.floor((copper % 10000) / 100)
  local cop = copper % 100
  if gold > 0 then return ("%s%dg %ds %dc"):format(sign, gold, silver, cop) end
  if silver > 0 then return ("%s%ds %dc"):format(sign, silver, cop) end
  return ("%s%dc"):format(sign, cop)
end

-- ---------------------------------------------------------------------------
-- Pricing — Auctionator's public API, soft dependency. Returns copper or nil.
-- ---------------------------------------------------------------------------
local function ahPrice(itemId)
  if not (Auctionator and Auctionator.API and Auctionator.API.v1 and Auctionator.API.v1.GetAuctionPriceByItemID) then
    return nil
  end
  local ok, price = pcall(Auctionator.API.v1.GetAuctionPriceByItemID, ADDON, itemId)
  if ok and price and price > 0 then return price end
  return nil
end

-- ---------------------------------------------------------------------------
-- Tooltip
-- ---------------------------------------------------------------------------
local function isDisenchantable(link)
  local _, _, _, equipLoc, _, classId, subclassId = GetItemInfoInstantCompat(link)
  if not equipLoc or equipLoc == "" then return false end
  if equipLoc == "INVTYPE_BAG" or equipLoc == "INVTYPE_QUIVER" or equipLoc == "INVTYPE_AMMO" then return false end
  return classId == CLASS_WEAPON or classId == CLASS_ARMOR
end

-- Runs on every item tooltip, not just disenchantable ones — any error here
-- (not just in the profit math) would otherwise get silently eaten by the
-- tooltip framework's own error boundary on every single hover, which is
-- exactly what was flooding the "too many errors in your UI" banner. So the
-- whole handler is pcall-wrapped, not just the part that does the pricing.
local function onTooltipSetItemImpl(tooltip)
  if type(tooltip.GetItem) ~= "function" then return end
  local _, link = tooltip:GetItem()
  if not link then return end
  if not isDisenchantable(link) then return end
  local itemId = GetItemInfoInstantCompat(link)
  if not itemId or not db then return end

  local entry = db.yields[itemId]
  local label = "Disenchant"
  if not entry or entry.n == 0 then
    -- Never personally disenchanted this exact item — borrow the average
    -- yield from everything else seen in its quality/item-level bucket,
    -- since that's what actually determines the result in WoW. Only a
    -- fallback: your own data on this exact item always wins.
    local _, _, quality, ilvl = GetItemInfoCompat(itemId)
    local key = bucketKey(quality, ilvl)
    entry = key and db.buckets[key]
    if entry and entry.n > 0 then
      label = ("Disenchant (predicted, %d similar item%s)"):format(entry.n, entry.n == 1 and "" or "s")
    else
      -- No data for this exact quality+level bracket yet — fall back to
      -- everything of the same quality regardless of level. Rougher (item
      -- level shifts which dust/essence/shard tier you get within a
      -- quality), but still beats nothing once at least one bracket's
      -- been seen.
      entry = quality and db.qualityBuckets[quality]
      if not entry or entry.n == 0 then trace("no bucket or quality-bucket data"); return end
      label = ("Disenchant (rough predict, %d item%s, other levels)"):format(entry.n, entry.n == 1 and "" or "s")
    end
  end

  -- Only show a number once every mat has a known price and the item
  -- itself has a buy price — a partial total would be misleading, and
  -- showing nothing beats showing a wrong number.
  local total = 0
  for matId, mat in pairs(entry.mats) do
    local price = ahPrice(matId)
    trace("mat", matId, "price=", price)
    if not price then return end
    total = total + (mat.count / entry.n) * price
  end

  local buy = ahPrice(itemId)
  trace("item buy price=", buy)
  if not buy then return end

  local profit = total - buy
  local sign = profit >= 0 and "+" or "-"
  local color = profit >= 0 and "|cff55dd55" or "|cffdd5555"
  tooltip:AddLine(("%s: %s%s%s|r"):format(label, color, sign, coinText(math.abs(profit))))
  tooltip:Show()
end

local function onTooltipSetItem(tooltip)
  local ok, err = pcall(onTooltipSetItemImpl, tooltip)
  if not ok then print("|cffff0000[RBDE ERROR]|r", err) end
end

-- Modern clients dropped GameTooltip's OnTooltipSetItem script in favor of
-- TooltipDataProcessor (confirmed live: HookScript on it now errors on load,
-- which silently killed everything below it, slash command included).
if TooltipDataProcessor and TooltipDataProcessor.AddTooltipPostCall and Enum and Enum.TooltipDataType then
  TooltipDataProcessor.AddTooltipPostCall(Enum.TooltipDataType.Item, onTooltipSetItem)
else
  GameTooltip:HookScript("OnTooltipSetItem", onTooltipSetItem)
  if ItemRefTooltip then ItemRefTooltip:HookScript("OnTooltipSetItem", onTooltipSetItem) end
end

-- ---------------------------------------------------------------------------
-- Debug / inspection
-- ---------------------------------------------------------------------------
SLASH_ROSTERBOOKDISENCHANT1 = "/rbde"
SlashCmdList.ROSTERBOOKDISENCHANT = function()
  if not db then loadSettings() end
  local n = 0
  for _ in pairs(db.yields) do n = n + 1 end
  print(("|cff9c8fe6RosterBook Disenchant|r: learned yields for %d item%s."):format(n, n == 1 and "" or "s"))
end
