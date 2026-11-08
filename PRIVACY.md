# Privacy

**The add-on** saves information about your own characters (name, realm, level, gold, items, professions, reputations, mail count and so on) into WoW's normal SavedVariables file on your PC. It sends nothing anywhere.

**The desktop app** reads that file and shows it:

- in its own window on your PC, and
- on phones or tablets on your local network that you have approved with a pairing code.

If you turn on **item pictures**, the app also reads the icons your characters' items use from the game's own files on your PC. It never changes those files. It keeps copies in your user cache folder (`Rosterbook/icons`) and shows them only to you and your connected devices.

It runs no account, analytics, advertising or tracking, and has no server of ours to send anything to. The only internet request it makes is checking GitHub for a newer version of itself (only in builds with updates enabled). That request sends the app's version and your system type, as any download would.

Settings, including the list of approved devices, are kept in your user config folder. Uninstalling the app and deleting that folder removes everything it stored. Deleting `Rosterbook.lua` from WoW's `SavedVariables` folders removes what the add-on recorded.
