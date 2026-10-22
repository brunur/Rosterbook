// Alt Ledger's own item icons: simple line drawings chosen by item category
// (class / subclass / equip slot recorded by the add-on). No game artwork.

(() => {
'use strict';

const GLYPHS = {
  box: '<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M4 7.5l8 4.5 8-4.5M12 12v9"/>',
  bag: '<path d="M5.5 9h13l-1.2 11.2a1 1 0 0 1-1 .8H7.7a1 1 0 0 1-1-.8z"/><path d="M9 9V7a3 3 0 0 1 6 0v2"/>',
  potion: '<path d="M10 3h4M10.5 3v5.2L6 16a3 3 0 0 0 2.6 4.5h6.8A3 3 0 0 0 18 16l-4.5-7.8V3"/><path d="M7.6 14h8.8"/>',
  food: '<circle cx="9.5" cy="9.5" r="5.5"/><path d="M13.5 13.5l5 5M17 20.5l1.5-2 2-1.5"/>',
  bandage: '<rect x="3.5" y="8.5" width="17" height="7" rx="3.5" transform="rotate(-40 12 12)"/><path d="M12 10.3v3.4M10.3 12h3.4"/>',
  scroll: '<path d="M8 4h9a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H8"/><path d="M8 4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2M10 9h6M10 12h6M10 15h4"/>',
  recipe: '<path d="M8 4h9a2 2 0 0 1 2 2v9M8 4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h5M10 9h6M10 12h4"/><circle cx="17" cy="18" r="2.5"/>',
  cloth: '<ellipse cx="7" cy="10" rx="3" ry="6"/><path d="M7 4h10a3 6 0 0 1 0 12H7"/><path d="M10 16l-2 4h12l-3-4"/>',
  leather: '<path d="M6 4c2 1.3 4 1.3 6 0 2 1.3 4 1.3 6 0-1 3 0 6 1 8-1 3-2 5-1 8-2-1.2-4-1.2-6 0-2-1.2-4-1.2-6 0 1-3 0-5-1-8 1-2 2-5 1-8z"/>',
  ore: '<path d="M4 15l4-8 6-2 6 5-1 8-8 2z"/><path d="M8 7l3 6 9-3M11 13l-3 7"/>',
  herb: '<path d="M5 19c0-8 6-14 14-14 0 8-6 14-14 14z"/><path d="M5 19l9-9M10 14h3.5M10 14v-3.5"/>',
  gem: '<path d="M6 9l3-4h6l3 4-6 11z"/><path d="M6 9h12M9 5l3 4 3-4M9 9l3 11 3-11"/>',
  dust: '<path d="M12 3v5M12 16v5M3 12h5M16 12h5"/><path d="M12 8l1 3 3 1-3 1-1 3-1-3-3-1 3-1z"/>',
  cog: '<circle cx="12" cy="12" r="3"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M5.6 18.4l2.1-2.1M16.3 7.7l2.1-2.1"/><circle cx="12" cy="12" r="6.5"/>',
  sword: '<path d="M19.5 4.5L9 15M14.5 4.5h5v5"/><path d="M6.5 12.5l5 5M8 16l-4 4"/>',
  dagger: '<path d="M18 6l-7 7M15 5h4v4"/><path d="M8.5 11.5l4 4M10 14l-5 5"/>',
  axe: '<path d="M6 21L16 7"/><path d="M13 4.5c3.5-1.5 7 .5 7.5 4.5L15 11c-.8-2.4-1.2-4.4-2-6.5z"/>',
  mace: '<path d="M5 20l8-8"/><circle cx="15.5" cy="8.5" r="4"/><path d="M15.5 3v1.5M21 8.5h-1.5M19.4 4.6l-1 1"/>',
  staff: '<path d="M6 21L17.5 6.5"/><circle cx="19" cy="5" r="2.2"/><path d="M14.5 6.5l3 3"/>',
  bow: '<path d="M7 3c7 3 9.5 11 6 18M7 3l6 18"/><path d="M3 12h16M16 9l3 3-3 3"/>',
  gun: '<path d="M3 9h17v4H10l-2 5H5l1-5H3z"/><path d="M16 9V7"/>',
  wand: '<path d="M5 19L15 9"/><path d="M17 3l.9 2.1L20 6l-2.1.9L17 9l-.9-2.1L14 6l2.1-.9z"/>',
  shield: '<path d="M12 3l7 3v5c0 5-3 8.2-7 10-4-1.8-7-5-7-10V6z"/><path d="M12 7v10"/>',
  helm: '<path d="M5 16a7 7 0 0 1 14 0v3H5z"/><path d="M8.5 15.5h7M12 5v4"/>',
  chest: '<path d="M8 4L4 7l2 4 2-1v10h8V10l2 1 2-4-4-3c-.8 1.6-2.2 2.5-4 2.5S8.8 5.6 8 4z"/>',
  legs: '<path d="M7 4h10l1 16h-4.5L12 10l-1.5 10H6z"/><path d="M7 7h10"/>',
  boots: '<path d="M8 3.5h5v9.5l6 3v4H6l2-4z"/><path d="M6 17.5h13"/>',
  gloves: '<path d="M8 21v-5l-3-4.5 1.6-1.2L9 13V5.5a1 1 0 0 1 2 0V12V4.5a1 1 0 0 1 2 0V12V5.5a1 1 0 0 1 2 0V13l1-2.5a1 1 0 0 1 1.9.6L17 16l-1 5"/>',
  belt: '<path d="M3 10h18v4H3z"/><rect x="9.5" y="8.5" width="5" height="7" rx="1"/>',
  cloak: '<path d="M8 4h8l3 16-7-2.5L5 20z"/><path d="M8 4l4 3 4-3"/>',
  ring: '<circle cx="12" cy="14.5" r="5.5"/><path d="M10 4h4l1.2 2.6L12 9 8.8 6.6z"/>',
  necklace: '<path d="M5 4c0 7 3 10.5 7 10.5S19 11 19 4"/><circle cx="12" cy="17.5" r="2.5"/>',
  trinket: '<circle cx="12" cy="12" r="8"/><path d="M12 7.5l1.3 2.9 3.2.3-2.4 2.1.7 3.1L12 14.2l-2.8 1.7.7-3.1-2.4-2.1 3.2-.3z"/>',
  orb: '<circle cx="12" cy="10" r="6"/><path d="M8.5 20h7M10 16l-.8 4M14 16l.8 4"/>',
  quest: '<circle cx="12" cy="12" r="9"/><path d="M12 7v6.5M12 16.5v.5"/>',
  key: '<circle cx="8" cy="12" r="4"/><path d="M12 12h9M18 12v3M21 12v3"/>',
  coin: '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="4.5"/>',
  arrow: '<path d="M4 20L19 5M14 4h6v6M4 16v4h4"/>',
  paw: '<circle cx="7" cy="10" r="1.8"/><circle cx="10.5" cy="6.5" r="1.8"/><circle cx="14.5" cy="6.5" r="1.8"/><circle cx="18" cy="10" r="1.8"/><path d="M8.5 17c0-3 1.6-5.5 3.5-5.5s3.5 2.5 3.5 5.5c0 2-1.5 3-3.5 2.2-2 .8-3.5-.2-3.5-2.2z"/>',
};

const SPRITE = `<svg xmlns="http://www.w3.org/2000/svg" style="display:none" aria-hidden="true">${
  Object.entries(GLYPHS).map(([k, v]) => `<symbol id="i-${k}" viewBox="0 0 24 24">${v}</symbol>`).join('')
}</svg>`;

const ARMOR_BY_SLOT = {
  INVTYPE_HEAD: 'helm', INVTYPE_NECK: 'necklace', INVTYPE_SHOULDER: 'chest', INVTYPE_BODY: 'chest',
  INVTYPE_CHEST: 'chest', INVTYPE_ROBE: 'chest', INVTYPE_WAIST: 'belt', INVTYPE_LEGS: 'legs',
  INVTYPE_FEET: 'boots', INVTYPE_WRIST: 'gloves', INVTYPE_HAND: 'gloves', INVTYPE_FINGER: 'ring',
  INVTYPE_TRINKET: 'trinket', INVTYPE_CLOAK: 'cloak', INVTYPE_SHIELD: 'shield', INVTYPE_HOLDABLE: 'orb',
  INVTYPE_TABARD: 'chest', INVTYPE_RELIC: 'trinket',
};

// Glyph name for an item, from Enum.ItemClass / subclass / equip slot.
function glyphFor(item) {
  const c = item.classId;
  const s = item.subclassId;
  switch (c) {
    case 0: return { 1: 'potion', 2: 'potion', 3: 'potion', 4: 'scroll', 5: 'food', 6: 'dust', 7: 'bandage' }[s] || 'potion';
    case 1: case 11: return 'bag';
    case 2: return { 0: 'axe', 1: 'axe', 2: 'bow', 3: 'gun', 4: 'mace', 5: 'mace', 6: 'staff', 7: 'sword', 8: 'sword',
      10: 'staff', 13: 'mace', 15: 'dagger', 16: 'dagger', 18: 'bow', 19: 'wand', 20: 'staff' }[s] || 'sword';
    case 3: return 'gem';
    case 4: return ARMOR_BY_SLOT[item.equipLoc] || (s === 6 ? 'shield' : 'chest');
    case 5: return 'dust';
    case 6: return 'arrow';
    case 7: return { 1: 'cog', 2: 'cog', 3: 'cog', 4: 'gem', 5: 'cloth', 6: 'leather', 7: 'ore', 8: 'food',
      9: 'herb', 10: 'dust', 12: 'dust' }[s] || 'box';
    case 8: return 'dust';
    case 9: return 'recipe';
    case 12: return 'quest';
    case 13: return 'key';
    case 15: return s === 2 || s === 5 ? 'paw' : 'box';
    case 16: return 'scroll';
    case 17: return 'paw';
    case 18: return 'coin';
    case 19: return 'cog';
    default:
      // Gear without a recorded class (older add-on data): fall back to the slot.
      return ARMOR_BY_SLOT[item.equipLoc] || 'box';
  }
}

function glyphSvg(item) {
  return `<svg class="gi" aria-hidden="true"><use href="#i-${glyphFor(item)}"/></svg>`;
}

document.body.insertAdjacentHTML('afterbegin', SPRITE);
window.AL_ICONS = { glyphFor, glyphSvg, GLYPHS };
})();
