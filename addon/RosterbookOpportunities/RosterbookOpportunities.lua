--[[
  RosterBook Opportunities — a passive data holder, no logic of its own.

  WoW add-ons can't make network requests, so there's no way for the
  desktop app to hand data to a running game client directly. Instead the
  desktop app writes this add-on's own SavedVariables file on disk
  (`RosterbookOpportunitiesDB`, ranked AH/disenchant opportunities from
  `opportunities::rank_disposals`), and WoW reads it back in the normal
  way at the next login or /reload. This add-on's only job is to declare
  that file and expose it to Rosterbook_UI's Opportunities tab; it is
  never written to from Lua.
]]

RosterbookOpportunitiesDB = RosterbookOpportunitiesDB or {}

RosterbookOpportunities = {
  GetRows = function() return RosterbookOpportunitiesDB.rows or {} end,
  GetGeneratedAt = function() return RosterbookOpportunitiesDB.generatedAt end,
}
