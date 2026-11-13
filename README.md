# RenamerAnime v2.0

A lightweight, cross-platform GUI + CLI utility (Windows 10/11, Fedora KDE Plasma Wayland, and Android) powered by a single shared Rust core (`renamer-core`), **Tauri v2**, and **React + TypeScript**. It parses messy anime/donghua release names, cleans scene/codec tags via data-driven JSON rules, queries **AniList GraphQL API** with adaptive rate limiting and persistent caching, and previews/applies safe reversible renames.

## ✨ v2.0 Capabilities & Architecture

- **Shared Rust Core (`crates/renamer-core`):**
  - Data-driven tag cleanup (`rules/default_rules.json`) stripping release groups (`[Judas]`, `[neoHEVC]`, `[Anime Time]`, `-EMBER`), quality/codec/source/audio tags (`1080p`, `HEVC`, `x265`, `10bit`, `WEB-DL`, `BD`, `Dual Audio`), CRC32 hashes, version markers (`v2`), and `COMPLETE`/`Batch` keywords while preserving season numbers (`(Season 1)`, `(Seasons 1-2)`), years, episode numbers, `+ Special / OVA / Movie` notes, and file extensions.
  - Naming modes: **English Title Only** (`In-Spectre (Season 2)`), **Prepend** (`(In-Spectre) Kyokou Suiri (Season 2)`), and **Append** (`Kyokou Suiri (In-Spectre) (Season 2)`), plus Option A episode formatting.
  - Adaptive AniList GraphQL client (`Page(perPage: 5)` batched queries, `X-RateLimit-*` pacing, `Retry-After` exponential backoff, and CJK fallback).
  - Persistent local cache (`title_cache.json`, `processed_items.json`, `manual_overrides.json`) and atomic bottom-up execution with one-click JSON transaction rollback (`transactions/latest.json`).
- **Cross-Platform Desktop & Mobile (`crates/renamer-gui` + `ui/`):**
  - Desktop GUI for Windows 10/11 & Fedora Linux (with `WEBKIT_DISABLE_DMABUF_RENDERER=1` Wayland/KDE Plasma guard).
  - **Android Storage Access Framework (`content://` URIs):** `build_virtual_rename_plan` + `SafBridge.kt` (`ACTION_OPEN_DOCUMENT_TREE`, `DocumentsContract.buildChildDocumentsUriUsingTree`, `DocumentsContract.renameDocument`) plus desktop-testable **SAF (content://) Demo** mode.
- **CLI & Legacy Wrapper (`crates/renamer-cli` + `anime.sh`):**
  - `renamer-cli` subcommands: `run`, `scan`, `undo`, `history`, `override`, `parse`, `demo-library`.
  - `anime.sh` delegates to `renamer-cli` automatically (use `--legacy` for original v1.0 Bash behavior).

## 📦 Packaging & Release Scripts

- **Windows (`x86_64`):** Run `.\packaging\windows\package-release.ps1` to generate `dist\RenamerAnime-v2.0.0-windows-x64.zip`.
- **Fedora Linux (`x86_64` / KDE Wayland):** Run `./packaging/linux/package-fedora.sh` to build `.tar.gz` (and `.rpm` when `rpmbuild` is installed).
- **Android APK (SAF `content://`):** Requires Android SDK + NDK (API 34+); run `.\packaging\android\build-android-apk.ps1` or trigger the GitHub Actions workflow in `.github/workflows/release.yml`.

---

## Legacy v1.0 Bash Script Documentation

A resilient, zero-configuration Bash utility designed for Linux systems that automatically parses messy anime and donghua release names (Romaji, Pinyin, or CJK), queries the **AniList GraphQL API**, and prepends the official English title to the beginning of the folder or file name.

---

## ✨ Features

* **Automatic English Title Prepending:** Transforms release names like `Nokemono-tachi no Yoru S01 1080p WEBRip AAC x265-EMBER` into `(The Tale of the Outcasts) Nokemono-tachi no Yoru S01 1080p WEBRip AAC x265-EMBER`.
* **Japanese Anime & Chinese Donghua Support:** Powered by AniList's GraphQL API, natively indexing both Japanese anime and Chinese donghua (Bilibili/Tencent releases), with dual-stage CJK fallback queries.
* **Pre-Flight Network & Dependency Diagnostics:** Validates CLI dependencies (`curl`, `jq`, `sed`) and performs an upfront HTTP status check against the API before scanning the filesystem.
* **Real-Time Terminal Telemetry:** Displays step-by-step tree output (`├─`, `└─`) with dynamic `[LOADING]` indicators, HTTP status reporting, and final execution summaries.
* **In-Memory Title Caching:** Uses Bash associative arrays (`declare -A`) to cache API responses during execution, preventing redundant API calls across multi-season or multi-episode directories.
* **Idempotent & Re-Run Safe:** Automatically detects and skips folders that have already been prepended, already have matching English titles, or belong to the system ignore list (`AnyDesk`, `.git`, `node_modules`).
* **Dry-Run Preview Mode:** Supports `--dry-run` to preview all regex extractions, API lookups, and target filenames without modifying the disk.

---

## 🛠️ Prerequisites

Ensure the following standard utilities are installed on your Linux distribution:

* `bash` (v4.2+ for associative array support)
* `curl`
* `jq`
* `sed`

**Fedora / RHEL:**
```bash
sudo dnf install curl jq sed
```

**Ubuntu / Debian:**
```bash
sudo apt install curl jq sed
```

**Arch Linux:**
```bash
sudo pacman -S curl jq sed
```

---

## 🚀 Usage

1. Place `anime.sh` inside your anime library directory (e.g., `~/Videos`) and make it executable:
   ```bash
   chmod +x anime.sh
   ```

2. **Preview Changes (Recommended):** Run with the `--dry-run` flag to verify API matches and preview new names without renaming anything on disk:
   ```bash
   ./anime.sh --dry-run
   ```

3. **Execute Renaming:** Run the script without arguments to rename folders and files:
   ```bash
   ./anime.sh
   ```

### Example Terminal Output
```text
[INIT] Working directory: /home/user/Videos
[CHECK] Verifying required CLI tools (curl, jq, sed)... OK
[CHECK] Testing AniList GraphQL API (graphql.anilist.co)... ONLINE (HTTP 200)
------------------------------------------------------------
[IGNORE] Skipping excluded item: 'AnyDesk'

>>> Processing: 'Nokemono-tachi no Yoru S01 1080p WEBRip AAC x265-EMBER'
    ├─ Cleaning title... Extracted query: 'Nokemono-tachi no Yoru'
    ├─ [API OK] AniList ID: 151679 | Romaji: 'Nokemono-tachi no Yoru' | English: 'The Tale of the Outcasts'
    └─ [RENAMED] -> '(The Tale of the Outcasts) Nokemono-tachi no Yoru S01 1080p WEBRip AAC x265-EMBER'

============================================================
Summary: 1 renamed | 1 skipped | 0 errors
```

---

## 🏗️ Architectural Choices (What Was Chosen & Why)

| Component / Decision | What Was Chosen | Why It Was Chosen |
| :--- | :--- | :--- |
| **Primary Metadata Engine** | **AniList GraphQL API** (`https://graphql.anilist.co`) over Jikan REST API (`api.jikan.moe`)  | 1. **Network Reachability:** Jikan's origin IP (`135.181.39.91` on Hetzner) is frequently blackholed or throttled by regional ISPs, whereas AniList routes cleanly through Cloudflare CDN.<br>2. **Donghua Indexing:** MyAnimeList (Jikan) lacks many Chinese animations (Donghua) or restricts them to obscure Pinyin entries. AniList natively indexes Bilibili/Chinese releases alongside Japanese anime.<br>3. **Structured GraphQL Payloads:** Allows requesting exact fields (`romaji`, `english`, `native`, `synonyms`) in a single lightweight POST request. |
| **Transport Layer Enforcement** | **`curl -4`** with explicit `--connect-timeout` & custom `User-Agent` | Dual-stack networks with broken IPv6 routing cause `curl` to hang while attempting `AAAA` record handshakes. Forcing IPv4 (`-4`) with strict timeouts (`6s` connect, `12s` max) guarantees fast execution. A Mozilla `User-Agent` prevents Cloudflare bot rules from dropping bare `curl` requests. |
| **Title Placement Strategy** | **Prepending `(${eng_title})`** to the start of the filename | Placing the English title at the start improves alphabetical sorting and immediate visual scanning in terminal (`ls`) and GUI file managers (Dolphin/Nautilus), whereas appended titles get cut off at the end of long release strings. |
| **Bracket Sanitization Logic** | **Asymmetric Bracket Stripping** (Strip leading `[...]`, preserve leading `(...)`) | Release groups almost universally use square brackets at the start of filenames (e.g., `[SubsPlease]`, `[Cortyx]`). By only stripping leading `[...]` during title extraction and reserving leading `(...)` for our prepended English titles, the script achieves idempotency (`^\([^)]+\)`), preventing recursive renaming on subsequent runs. |
| **Bilingual / CJK Fallback Search** | **Two-Stage Query Retry** (Full cleaned string $\rightarrow$ Native CJK characters only) | Releases from groups like `[Prejudice-Studio]` often combine Chinese characters and literal English translations in the filename (e.g., `记忆管理局 False Memory`). If a combined search returns `HTTP 404`, the script strips ASCII characters and retries using only the native CJK string (`记忆管理局`), ensuring high match rates. |
| **Error Handling Model** | **`set -uo pipefail`** (Omitting `-e`) with explicit HTTP status capture | Using `set -e` in a network-heavy loop causes the entire script to crash silently if a single `curl` timeout or `jq` parse error occurs. Capturing `%{http_code}` into a variable and routing the JSON payload to a temporary file (`mktemp`) allows the script to log API errors gracefully and continue processing the remaining folders. |

---

## 🐛 Troubleshooting & Errors Faced During Development

### 1. Silent Hangs Followed by "No Files Need to Be Renamed"
* **Symptom:** Running the original script caused the terminal to freeze for a long time before finishing without renaming any eligible folders.
* **Diagnostic:** Running a verbose IPv4 connectivity test revealed a TCP timeout at the ISP/routing level:
  ```text
  Host api.jikan.moe:443 was resolved.
  IPv4: 135.181.39.91
  Trying 135.181.39.91:443...
  curl: (28) Connection timed out after 5000 milliseconds
  ```
* **Root Cause:** The original script used `curl -sG` without `--connect-timeout` or `--max-time` against `api.jikan.moe` . Because the host IP (`135.181.39.91`) was blackholed on the network, every folder triggered a default OS socket timeout. Once `curl` timed out and returned an empty string, `jq` evaluated `.data[0].title_english` as empty , causing every folder to hit the `[NO MATCH]` branch or exit silently due to `set -euo pipefail` .
* **Fix:** Migrated the backend to `https://graphql.anilist.co`, added an upfront startup connectivity check, and enforced `--connect-timeout 6 --max-time 12` on all `curl` invocations.

### 2. False `HTTP 000000` Status Code in Diagnostics
* **Symptom:** Early diagnostic checks printed `FAILED (HTTP 000000)` when testing an unreachable endpoint.
* **Root Cause:** When `curl -w "%{http_code}"` fails to connect, `curl` prints `000` to `stdout` and exits with return code `28`. Combining that with an inline fallback `|| echo "000"` concatenated a second `000` onto the output string (`000` + `000` = `000000`).
* **Fix:** Replaced `|| echo "000"` with `|| true` so `curl`'s native `000` status code is captured cleanly without string duplication.

### 3. `HTTP/2 404` When Probing AniList with `curl -I`
* **Symptom:** Testing `https://graphql.anilist.co` using `curl -4 -I` returned `HTTP/2 404`.
* **Root Cause:** `curl -I` sends an HTTP `HEAD` request. GraphQL endpoints only accept `POST` requests with a JSON query body (`Content-Type: application/json`).
* **Fix:** Updated the pre-flight health check to send a minimal GraphQL `POST` probe (`{"query":"{Page(perPage:1){media(search:\"naruto\",type:ANIME){id}}}"}`), which accurately validates both network routing and GraphQL engine readiness (`HTTP 200`).

### 4. Uncleaned Episode Ranges & Non-Anime Folders
* **Symptom:** Folders like `[Prejudice-Studio] 记忆管理局 False Memory [01-07][Bilibili WEB-DL...]` retained `[01-07]` in the search query, and utility folders like `AnyDesk/` were queried as anime titles.
* **Root Cause:** The original `sed` pipeline only matched `S01` or `E01` season/episode formats  and lacked a directory exclusion list.
* **Fix:** Added a dedicated regex pass (`s/[[:space:]._-]*\[[0-9]{1,3}[-~][0-9]{1,3}\].*//`) to strip bracketed episode ranges (`[01-07]`, `[01~12]`) and implemented an `IGNORE_LIST` array (`AnyDesk`, `.git`, `node_modules`).

### 5. Re-Run Collision Risk When Switching from Append to Prepend
* **Symptom:** Moving `(${eng_title})` to the start of the filename risked having the script strip the newly added English title on future runs.
* **Root Cause:** Step 1 of `clean_anime_name()` originally used `s/^(\[[^]]+\]|\([^)]+\))[[:space:]._-]*//` , which stripped both `[square brackets]` and `(parentheses)` from the start of a filename .
* **Fix:** Restricted Step 1 of `clean_anime_name()` to only strip leading square brackets (`^\[[^]]+\]`), and added an explicit guard (`if [[ "$name_part" =~ ^\([^)]+\)[[:space:]]+ ]]`) to immediately skip items that already have a parenthesized title prepended.
