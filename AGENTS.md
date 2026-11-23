# TruckCraft contributor instructions

TruckCraft is a native Rust save editor for Euro Truck Simulator 2 and American
Truck Simulator. Its displayed app and Cargo package versions are **1.0.4**.
The simulator compatibility target is **ETS2 and ATS 1.62.x**.

## Repository and provenance

- Describe TruckCraft as a native Rust save editor. Retain attribution for
  adapted third-party code in `scs-codec`.
- Project, issue and PR links use https://github.com/chrissts/TruckCraft.
- Retain `LICENSE`, `NOTICE`, dependency licenses and asset provenance. Game logos
  remain SCS Software assets; they are not Apache-licensed application artwork.
- Work in this repository. Do not silently copy changes back into the legacy C#
  repository. Publishing, pushing and sending messages require user authorization.
- Keep GitHub private until full testing is complete. Do not create a public
  repository, publish releases, or change visibility to public. Any authorized
  GitHub repository creation during this stage must explicitly use private visibility.

## Save integrity

- Preserve unknown units, fields, array order and game references.
- Stage edits, validate related changes as a transaction, and support Undo/Redo.
- Refuse a write if input files changed externally. Create a recovery copy before
  replacing any original file and require reload after an unverified write.
- Never use original user saves for write tests. Use synthetic data or temporary
  copies of opt-in private fixtures. Do not infer successful in-game loading from
  a parser round trip.
- Bound input sizes, allocation, decompression and graph traversal. Archive access
  is read-only; never execute content from saves, mods or game archives.
- Do not add telemetry or a native decoder DLL.
- In-app updates are allowed. Verify the authenticity and integrity of the
  update before installing it. Ask the user before replacing the app or
  restarting it, and preserve pending save edits.
- Keep macOS updates manual while builds are test artifacts. If a public
  Developer ID-signed and notarized `.app` is available later, evaluate
  Sparkle with signed update archives and an authenticated appcast. Verify
  installation and recovery on Apple Silicon and Intel hardware before shipping.
- Discover both local and OneDrive Documents game homes, the default Steam game
  installations, and Steam userdata remote homes (ETS2 app 227300; ATS app 270880).
  Keep custom-folder discovery for other libraries and redirected homes. Warn
  that Steam Cloud sync can conflict with edits, but allow editing and do not
  infer that sync is enabled merely from a Cloud path.

## Interface requirements

- No horizontal page scrolling. Reflow controls and wrap long text; keep all main
  tabs aligned with images. Test the narrowest supported window and sidebar.
- Coach is singular, next to Truck, and hidden for ATS. In ETS2, show its data/DLC
  status even when coach data is absent. Require an active saved coach journey
  before enabling coach edits.
- ATS shows Car in the same position. Require Road Trip DLC, an active saved car
  and a saved Road Trip journey before enabling Car edits. Keep its vehicle and
  job references separate from Truck and Coach.
- Put Headquarters immediately after Company name. Keep the large exact integer
  money input and quick amount buttons; never convert balances to floating point.
- Keep Save/Undo/Redo accessible and retain unapplied inputs across unrelated edits.
- Present number plates as readable text with colour pickers and visual controls
  for game formatting, not raw markup inputs. Preserve unknown formatting.
- Automatically load the last game's remembered save, keeping separate choices
  for ETS2 and ATS. Fall back to the newest discovered non-backup save and prompt
  for a save/profile folder when none can load. Never discard pending edits.
- Use English when language selection is absent, invalid or unsupported.
- Use TruckCraft branding and the app version. No DLC suffix in the title or About box;
  no personal liability sentence, old support email, or collapse arrows.
- Bundle the official game logos, skill badges, stock portraits and company
  logos in the default private build. Retain attribution and the original tab
  artwork's prompt provenance. Resolve artwork redistribution before posting
  the repository or its builds publicly.

## Add a language

1. Copy [en.json](crates/desktop/locales/en.json) to a new UTF-8 JSON file in the
   same directory, for example `de.json` or `pt-br.json`. The lowercase filename
   without `.json` is the saved language code. Set `name` to the language's own
   name as it should appear in Settings.
2. Under `messages`, keep the English keys exactly as written and translate the
   values. Omit untranslated entries to use English for those messages. Use
   recognizable in-game terms and preserve game codes and filenames such as
   `TruckCraft.log`.
3. Run `cargo test --locked -p truckcraft i18n`, rebuild, then choose the language
   in **Settings → Language**. Restart to verify it persists. Check tabs, buttons
   and dialogs at minimum window width for readability and missing characters.
   Submit your locale file in a pull request. The build discovers and embeds
   JSON locale files automatically.

To add a UI message, call `tr("English text")` and add the same text as both key
and value in `en.json`. Give translated windows and controls stable IDs. Leave
game data, file formats and diagnostic logs outside the translation catalog.
Formatted status messages and some dynamic labels still need catalog entries.

## Verification and release hygiene

Install the hooks with `pre-commit install`. Run `pre-commit run` against
staged files before committing and resolve any reported failures. GitHub Actions
runs the same hooks on files changed by a pull request.

Use Conventional Commits-style pull request titles, such as
`docs: add a support badge to the README` or `fix(updater): preserve pending edits`.

Run the checks appropriate to the change:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --release --workspace
```

`build-rust.ps1` supports a locally installed Rust/GNU toolchain and remaps local
paths out of executable metadata. Use `-CargoArguments` to run the checks above.
Private integration tests are ignored by default and take fixture paths from
environment variables. Keep those fixtures and raw output outside the repository.

Review screenshots visually. Documentation screenshots must use invented sample
profiles and contain no personal names, file paths, IDs or desktop content.
Inspect release files and decompressed source and application archives for private data. Never ship private fixtures, raw scan logs, profile
databases, settings, debug symbols, build directories or machine paths. Report
malware scan results accurately; a clean scan is not a guarantee of safety.

## Local testing

Keep game installations, fixture paths and raw evidence in local configuration
outside the checkout. For Windows GNU builds, supply the installed Rust and GNU
toolchain root to `build-rust.ps1 -ToolchainDirectory`. Use the configured GNU
linker with the Rust GNU toolchain. The wrapper applies build path remapping.

Set these environment variables for opt-in tests. Use copies of authorized
fixtures for testing and keep test artifacts outside the repository.

| Variable | Purpose |
| --- | --- |
| `TSSET_FIXTURE_ROOT` | Fixture root with `ETS2` and `ATS` subdirectories |
| `TSSET_COACH_FIXTURE` | Active ETS2 coach journey save |
| `TSSET_CAR_FIXTURE` | ATS profile with an active Road Trip journey |
| `TRUCKCRAFT_ETS2_GAME_DIR` | Installed ETS2 game directory for read-only checks |
| `TRUCKCRAFT_ATS_GAME_DIR` | Installed ATS game directory for read-only checks |

Run all opt-in checks with `cargo test --locked --workspace -- --ignored`.
Use `truckcraft.exe --isolated` to disable profile discovery, preference
persistence and activity logging during native testing. Open a disposable
save manually. Keep screenshots free of personal information.

The read-only archive inspector is in `crates/scs-codec/examples/inspect_archive.rs`.
The one-time game artwork importer is in
`crates/desktop/examples/import_game_art.rs`. It produces PNGs and a manifest.
The artwork catalog and asset provenance require a separate update when the
bundled artwork changes.

## Release validation

- Check both games with disposable profiles. Cover startup and discovery, pending
  edits, Save/Undo/Redo, profile operations, fleet and company edits, markets,
  convoy routes, and active Coach and Road Trip journeys.
- Check native layouts at 1180×720, including higher DPI and narrow sidebar
  sizes. Check translated controls, Settings persistence, About and the log
  viewer. Confirm the loaded save format appears in About.
- Load edited copies in each game. Record native UI and in-game results
  separately from automated geometry and save reloads.
- Verify a standalone build on a clean Windows machine. Scan the exact
  executable, inspect the decompressed archives, and exclude local SCS artwork
  from public source and executable packages. Keep raw evidence and caches outside releases.
