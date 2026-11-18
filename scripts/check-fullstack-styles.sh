#!/bin/sh
set -eu

server=${1:?usage: check-fullstack-styles.sh <server-binary> <public-dir>}
public_dir=${2:?usage: check-fullstack-styles.sh <server-binary> <public-dir>}
port=18089
tmp_dir=$(mktemp -d)
server_pid=

cleanup() {
    if [ -n "$server_pid" ]; then
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
    fi
    rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

IP=127.0.0.1 PORT=$port \
    DATABASE_URL="sqlite://$tmp_dir/test.db?mode=rwc" \
    DIOXUS_PUBLIC_PATH="$public_dir" \
    "$server" >"$tmp_dir/server.log" 2>&1 &
server_pid=$!

ready=false
for attempt in $(seq 1 60); do
    if wget -qO "$tmp_dir/page.html" "http://127.0.0.1:$port/"; then
        ready=true
        break
    fi
    if ! kill -0 "$server_pid" 2>/dev/null; then
        cat "$tmp_dir/server.log" >&2
        echo "Fullstack server exited before serving the page" >&2
        exit 1
    fi
    sleep 1
done
if [ "$ready" != true ]; then
    cat "$tmp_dir/server.log" >&2
    echo "Fullstack server did not become ready" >&2
    exit 1
fi

if grep -q 'This should be replaced by dx' "$tmp_dir/page.html"; then
    echo "Fullstack HTML contains Dioxus's unresolved stylesheet placeholder" >&2
    exit 1
fi

grep -oE 'href="[^"]+\.css([?][^"]*)?"' "$tmp_dir/page.html" \
    | sed -E 's/^href="([^"]+)"$/\1/' \
    | sort -u > "$tmp_dir/stylesheets.txt"
style_count=$(wc -l < "$tmp_dir/stylesheets.txt")
if [ "$style_count" -lt 7 ]; then
    echo "Expected at least 7 stylesheet links in fullstack HTML, found $style_count" >&2
    cat "$tmp_dir/page.html" >&2
    exit 1
fi

has_login_styles=false
while IFS= read -r stylesheet; do
    case "$stylesheet" in
        http://*|https://*) url=$stylesheet ;;
        /*) url="http://127.0.0.1:$port$stylesheet" ;;
        *) url="http://127.0.0.1:$port/$stylesheet" ;;
    esac
    name=$(printf '%s' "$stylesheet" | tr '/?' '__')
    if ! wget -qO "$tmp_dir/$name" "$url"; then
        echo "Unable to fetch stylesheet $stylesheet" >&2
        exit 1
    fi
    if [ ! -s "$tmp_dir/$name" ] || grep -qi '<html' "$tmp_dir/$name"; then
        echo "Stylesheet URL did not return CSS: $stylesheet" >&2
        exit 1
    fi
    if grep -q '\.login-screen' "$tmp_dir/$name"; then
        has_login_styles=true
    fi
done < "$tmp_dir/stylesheets.txt"

if [ "$has_login_styles" != true ]; then
    echo "Fetched stylesheets are missing the login screen rules" >&2
    exit 1
fi
