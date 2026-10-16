#!/usr/bin/env bash
set -uo pipefail
shopt -s nullglob

# Colors for terminal output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
    echo -e "${YELLOW}=== DRY RUN MODE: No files or folders will be renamed ===${NC}"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
USER_AGENT="Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0"

# Folders/files to ignore
IGNORE_LIST=("AnyDesk" ".git" "node_modules")

echo -e "${CYAN}[INIT] Working directory: ${SCRIPT_DIR}${NC}"

# 1. Check required tools
echo -ne "${CYAN}[CHECK] Verifying required CLI tools (curl, jq, sed)... ${NC}"
for cmd in curl jq sed; do
    if ! command -v "$cmd" &>/dev/null; then
        echo -e "${RED}FAILED!${NC}"
        echo -e "${RED}[ERROR] Required command '$cmd' is not installed.${NC}" >&2
        exit 1
    fi
done
echo -e "${GREEN}OK${NC}"

# 2. Check AniList GraphQL API Connectivity
echo -ne "${CYAN}[CHECK] Testing AniList GraphQL API (graphql.anilist.co)... ${NC}"
ANILIST_HTTP=$(curl -4 -s -A "$USER_AGENT" -o /dev/null -w "%{http_code}" --connect-timeout 5 --max-time 10 \
    -X POST -H "Content-Type: application/json" -H "Accept: application/json" \
    -d '{"query":"{Page(perPage:1){media(search:\"naruto\",type:ANIME){id}}}"}' \
    "https://graphql.anilist.co" || true)

if [[ "$ANILIST_HTTP" == "200" ]]; then
    echo -e "${GREEN}ONLINE (HTTP 200)${NC}"
elif [[ "$ANILIST_HTTP" == "429" ]]; then
    echo -e "${YELLOW}RATE LIMITED (HTTP 429) - Proceeding with caution...${NC}"
else
    echo -e "${RED}FAILED (HTTP $ANILIST_HTTP)${NC}"
    echo -e "${RED}[ERROR] Cannot reach AniList API. Check your internet connection.${NC}" >&2
    exit 1
fi

echo "------------------------------------------------------------"

declare -A TITLE_CACHE
RENAMED_COUNT=0
SKIPPED_COUNT=0
ERROR_COUNT=0

clean_anime_name() {
    local raw="$1"
    local cleaned="$raw"

    # 1. Strip leading square-bracket release group tags like [SubsPlease] or [Cortyx]
    cleaned="$(echo "$cleaned" | sed -E 's/^\[[^]]+\][[:space:]._-]*//')"

    # 2. Strip trailing bracketed hashes/tags like [1080p][ABCD1234]
    cleaned="$(echo "$cleaned" | sed -E 's/(\[[^]]+\]|\([^)]+\))+[[:space:]._-]*$//')"

    # 3. Strip episode range brackets like [01-07] or [01~12] and everything after
    cleaned="$(echo "$cleaned" | sed -E 's/[[:space:]._-]*\[[0-9]{1,3}[-~][0-9]{1,3}\].*//')"

    # 4. Cut everything from Season/Episode markers, resolution, or source/codec tags onward
    cleaned="$(echo "$cleaned" | sed -E 's/[[:space:]._-]+(S[0-9]{1,2}(E[0-9]{1,3})?|Season[[:space:]._-]*[0-9]+|E[0-9]{1,3}|-[[:space:]]*S?[0-9]{1,3}([[:space:].v\[]|$)|[0-9]{3,4}p|4K|UHD|WEBRip|WEB-DL|WEB|BluRay|BDRip|BD|HDTV|AAC|FLAC|AC3|EAC-3|Opus|x264|x265|H\.?264|H\.?265|HEVC|AV1|10[[:space:]]*bits?|8[[:space:]]*bits?|Dual[[:space:]._-]*Audio|Multi[[:space:]._-]*Subs?).*//I')"

    # 5. Replace dots and underscores with spaces, and collapse multiple spaces
    cleaned="$(echo "$cleaned" | sed -E 's/[._]+/ /g; s/[[:space:]]+/ /g; s/^ //; s/ $//')"

    echo "$cleaned"
}

sanitize_filename() {
    echo "$1" | sed -E 's/[:\/\\|]+/ - /g; s/[?*"<>]+//g; s/[[:space:]]+/ /g; s/^ //; s/ $//'
}

is_ignored() {
    local target="$1"
    for ignore in "${IGNORE_LIST[@]}"; do
        if [[ "${target,,}" == "${ignore,,}" ]]; then
            return 0
        fi
    done
    return 1
}

query_anilist() {
    local search_term="$1"
    local tmp_resp
    tmp_resp=$(mktemp)

    local gql_payload
    gql_payload=$(jq -n --arg q "$search_term" \
        '{"query": "query ($q: String) { Media(search: $q, type: ANIME) { id title { romaji english native } synonyms } }", "variables": {"q": $q}}')

    local code
    code=$(curl -4 -s -A "$USER_AGENT" -w "%{http_code}" -o "$tmp_resp" \
        --connect-timeout 6 --max-time 12 \
        -X POST -H "Content-Type: application/json" -H "Accept: application/json" \
        -d "$gql_payload" "https://graphql.anilist.co" || true)

    API_HTTP_CODE="$code"
    API_RESPONSE="$(cat "$tmp_resp")"
    rm -f "$tmp_resp"
}

for item in "$SCRIPT_DIR"/*; do
    base_name="$(basename "$item")"
    [[ "$base_name" == "$SCRIPT_NAME" ]] && continue

    if is_ignored "$base_name"; then
        echo -e "${YELLOW}[IGNORE]${NC} Skipping excluded item: '$base_name'"
        ((SKIPPED_COUNT++))
        continue
    fi

    echo -e "\n${BOLD}>>> Processing:${NC} '$base_name'"

    if [[ -d "$item" ]]; then
        name_part="$base_name"
        ext_part=""
    else
        if [[ "$base_name" == *.* ]]; then
            name_part="${base_name%.*}"
            ext_part=".${base_name##*.}"
        else
            name_part="$base_name"
            ext_part=""
        fi
    fi

    # Skip immediately if the item already starts with a parenthesized title from a previous run
    if [[ "$name_part" =~ ^\([^)]+\)[[:space:]]+ ]]; then
        echo -e "    └─ ${CYAN}[ALREADY PREPENDED]${NC} '$base_name' already has a title at the start."
        ((SKIPPED_COUNT++))
        continue
    fi

    echo -ne "    ├─ Cleaning title... "
    query_title="$(clean_anime_name "$name_part")"
    echo -e "Extracted query: ${CYAN}'$query_title'${NC}"

    if [[ -z "$query_title" || ${#query_title} -lt 2 ]]; then
        echo -e "    └─ ${YELLOW}[SKIP] Could not extract a valid search query.${NC}"
        ((SKIPPED_COUNT++))
        continue
    fi

    if [[ -v TITLE_CACHE["$query_title"] ]]; then
        eng_title="${TITLE_CACHE["$query_title"]}"
        echo -e "    ├─ ${GREEN}[CACHE HIT]${NC} Found cached title: '$eng_title'"
    else
        echo -ne "    ├─ ${YELLOW}[LOADING]${NC} Querying AniList API for '$query_title'..."
        query_anilist "$query_title"

        # If mixed Chinese/English string returns 404 (e.g. "记忆管理局 False Memory"), try searching just the CJK part
        if [[ "$API_HTTP_CODE" == "404" ]] && [[ "$query_title" =~ [^[:ascii:]] ]] && [[ "$query_title" =~ [A-Za-z] ]]; then
            cjk_only="$(echo "$query_title" | sed -E 's/[[:ascii:]]+//g; s/^[[:space:]]+//; s/[[:space:]]+$//')"
            if [[ -n "$cjk_only" ]]; then
                echo -ne "\r\033[K    ├─ ${YELLOW}[LOADING]${NC} Retrying AniList API with native title '$cjk_only'..."
                query_anilist "$cjk_only"
            fi
        fi

        echo -ne "\r\033[K"

        if [[ "$API_HTTP_CODE" == "404" ]]; then
            echo -e "    └─ ${YELLOW}[NO MATCH]${NC} Not found on AniList for '$query_title'."
            TITLE_CACHE["$query_title"]=""
            ((SKIPPED_COUNT++))
            continue
        elif [[ "$API_HTTP_CODE" != "200" ]]; then
            echo -e "    └─ ${RED}[API ERROR]${NC} HTTP $API_HTTP_CODE returned for '$query_title'. Skipping."
            ((ERROR_COUNT++))
            continue
        fi

        raw_eng="$(echo "$API_RESPONSE" | jq -r '.data.Media.title.english // empty' 2>/dev/null || true)"
        raw_def="$(echo "$API_RESPONSE" | jq -r '.data.Media.title.romaji // empty' 2>/dev/null || true)"
        api_id="$(echo "$API_RESPONSE" | jq -r '.data.Media.id // empty' 2>/dev/null || true)"

        echo -e "    ├─ ${GREEN}[API OK]${NC} AniList ID: ${api_id:-None} | Romaji: '${raw_def:-None}' | English: '${raw_eng:-None}'"

        eng_title="${raw_eng:-$raw_def}"
        eng_title="$(sanitize_filename "$eng_title")"
        TITLE_CACHE["$query_title"]="$eng_title"

        echo -ne "    ├─ ${CYAN}[WAIT]${NC} Respecting API rate limit (0.7s)..."
        sleep 0.7
        echo -ne "\r\033[K"
    fi

    if [[ -z "$eng_title" ]]; then
        echo -e "    └─ ${YELLOW}[NO MATCH]${NC} No valid title returned for '$query_title'."
        ((SKIPPED_COUNT++))
        continue
    fi

    if [[ "${query_title,,}" == "${eng_title,,}" ]]; then
        echo -e "    └─ ${CYAN}[ALREADY ENGLISH]${NC} Extracted title ('$query_title') matches English title ('$eng_title')."
        ((SKIPPED_COUNT++))
        continue
    fi

    if [[ "${name_part,,}" == *"${eng_title,,}"* ]]; then
        echo -e "    └─ ${CYAN}[ALREADY PRESENT]${NC} '$eng_title' is already in '$base_name'."
        ((SKIPPED_COUNT++))
        continue
    fi

    # Prepend English title to the start of the folder/file name
    new_name="(${eng_title}) ${name_part}${ext_part}"
    new_path="${SCRIPT_DIR}/${new_name}"

    if [[ -e "$new_path" ]]; then
        echo -e "    └─ ${RED}[CONFLICT]${NC} Target already exists: '$new_name'"
        ((ERROR_COUNT++))
        continue
    fi

    if [[ "$DRY_RUN" == true ]]; then
        echo -e "    └─ ${GREEN}[PREVIEW]${NC} Would rename to -> ${BOLD}'$new_name'${NC}"
        ((RENAMED_COUNT++))
    else
        echo -ne "    ├─ ${YELLOW}[LOADING]${NC} Renaming item on disk..."
        mv "$item" "$new_path"
        echo -ne "\r\033[K"
        echo -e "    └─ ${GREEN}[RENAMED]${NC} -> ${BOLD}'$new_name'${NC}"
        ((RENAMED_COUNT++))
    fi
done

echo -e "\n============================================================"
if [[ "$DRY_RUN" == true ]]; then
    echo -e "${BOLD}Summary (Dry Run):${NC} ${GREEN}${RENAMED_COUNT} to rename${NC} | ${YELLOW}${SKIPPED_COUNT} skipped${NC} | ${RED}${ERROR_COUNT} errors${NC}"
else
    echo -e "${BOLD}Summary:${NC} ${GREEN}${RENAMED_COUNT} renamed${NC} | ${YELLOW}${SKIPPED_COUNT} skipped${NC} | ${RED}${ERROR_COUNT} errors${NC}"
fi