#!/usr/bin/env bash
#
# Deploy the PhotoTools server from this Mac to the NAS.
#
#     ./deploy/deploy.sh --check      # look, change nothing
#     ./deploy/deploy.sh              # build, ship, start, confirm
#
# The image is built here and piped over SSH. There is no registry: nothing
# about this library or this build leaves the network, and the only credential
# involved is the SSH key you already use.
#
# Everything the script needs about the NAS beyond "where is it" is discovered
# rather than configured. A path typed into a file twice is a path that is
# wrong in one of them eventually.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

CONFIG="deploy/deploy.env"
# Must agree with deploy/docker-compose.yml.
IMAGE="masterphototools"
CONTAINER="MasterPhotoTools"
TAG="${PHOTOTOOLS_TAG:-latest}"

# The NAS is x86-64; this Mac is ARM. buildx cross-builds, which works and is
# slower than a native build — mostly in rawler, mozjpeg and aws-lc, which is
# why the dependency layer is cached separately in the Dockerfile.
PLATFORM="${PLATFORM:-linux/amd64}"
BUILDER="phototools"

# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------

if [ -t 1 ]; then
    BOLD=$'\033[1m'; DIM=$'\033[2m'; RED=$'\033[31m'; GREEN=$'\033[32m'
    YELLOW=$'\033[33m'; RESET=$'\033[0m'
else
    BOLD=''; DIM=''; RED=''; GREEN=''; YELLOW=''; RESET=''
fi

step() { printf '\n%s==> %s%s\n' "$BOLD" "$1" "$RESET"; }
ok()   { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$1"; }
warn() { printf '  %s!%s %s\n' "$YELLOW" "$RESET" "$1"; }
note() { printf '    %s%s%s\n' "$DIM" "$1" "$RESET"; }
die()  { printf '\n  %s✕ %s%s\n\n' "$RED" "$1" "$RESET" >&2; exit 1; }

CHECK_ONLY=false
[ "${1:-}" = "--check" ] && CHECK_ONLY=true

# ---------------------------------------------------------------------------
# Configuration — this side only
# ---------------------------------------------------------------------------

[ -f "$CONFIG" ] || die "No $CONFIG. Copy deploy/deploy.env.example to it and set NAS_SSH."

# shellcheck disable=SC1090
set -a; . "./$CONFIG"; set +a

[ -n "${NAS_SSH:-}" ] || die "NAS_SSH is not set in $CONFIG. It is anything ssh understands: pablo@nas.local, or a host from ~/.ssh/config."
REMOTE_DIR="${REMOTE_DIR:-/srv/phototools}"

# One connection, reused by every ssh and scp below.
#
# A NAS reached by password is the normal case — OMV sets up an account, not a
# key — and this script makes a dozen connections. Without multiplexing that is
# a dozen password prompts, which is the kind of thing that makes people paste
# their password into a script. The master is opened once, interactively, and
# everything after it rides the same socket; a key, if one is installed, simply
# means the prompt never appears.
#
# ConnectTimeout because the failure this guards against is a NAS that is asleep
# or renamed, and ssh's own default is to sit there for two minutes before
# saying so.
CONTROL_DIR="$(mktemp -d)"
SSH_OPTS=(-o "ControlPath=$CONTROL_DIR/cm" -o ConnectTimeout=10)

REMOTE_ENV=""
cleanup() {
    [ -n "$REMOTE_ENV" ] && rm -f "$REMOTE_ENV"
    ssh -o "ControlPath=$CONTROL_DIR/cm" -O exit "$NAS_SSH" 2>/dev/null || true
    rm -rf "$CONTROL_DIR"
}
trap cleanup EXIT

nas() { ssh "${SSH_OPTS[@]}" "$NAS_SSH" "$@"; }

# ---------------------------------------------------------------------------
# 1. Reach the NAS, and find out what it is
# ---------------------------------------------------------------------------

step "The NAS"

# Interactive, and the only place a password can be asked for. If one is
# wanted, it is ssh asking — not this script, which never sees it.
#
# The master stays up until cleanup closes it, not for a fixed time. Between
# the checks and the upload sits a cross-compile that runs far longer than any
# timeout worth choosing, with no ssh traffic at all; a master that expired in
# the middle would turn every remaining step into another password prompt.
# The keepalive stops the NAS dropping a connection that idle.
MASTER_OPTS=(-o ConnectTimeout=10 -o "ControlPath=$CONTROL_DIR/cm"
             -o ControlMaster=yes -o ControlPersist=yes -o ServerAliveInterval=60)
if ! ssh -o BatchMode=yes "${MASTER_OPTS[@]}" -fN "$NAS_SSH" 2>/dev/null; then
    note "no key installed for $NAS_SSH, so ssh will ask for its password once"
    ssh "${MASTER_OPTS[@]}" -fN "$NAS_SSH" \
        || die "Cannot connect to $NAS_SSH. Check the host and the username."
fi
nas true || die "Connected to $NAS_SSH, but it will not run a command."
ok "ssh $NAS_SSH"

REMOTE_ARCH="$(nas 'uname -m')"
case "$REMOTE_ARCH" in
    x86_64)  REMOTE_PLATFORM=linux/amd64 ;;
    aarch64) REMOTE_PLATFORM=linux/arm64 ;;
    *)       die "Unknown architecture $REMOTE_ARCH. Set PLATFORM in $CONFIG by hand." ;;
esac
ok "$(nas 'uname -sr') on $REMOTE_ARCH  ($REMOTE_PLATFORM)"

# Building for the wrong architecture produces an image that loads happily and
# dies on exec with a message about the binary format. Caught here instead.
if [ "$PLATFORM" != "$REMOTE_PLATFORM" ]; then
    die "Building $PLATFORM for a $REMOTE_PLATFORM machine. Fix PLATFORM in $CONFIG."
fi

nas 'command -v docker >/dev/null' || die "No docker on the NAS. Install it (OMV: omv-extras → Docker) and try again."

# Having the binary is not being allowed to use it. An OMV account is in
# `users`, not `docker`, and without this the script would build for twenty
# minutes and then fail at `docker load` with a socket permission error.
nas 'docker info >/dev/null 2>&1' \
    || die "$NAS_SSH has docker but may not use it. As root on the NAS: usermod -aG docker $(nas 'id -un'), then run this again."
ok "docker $(nas 'docker version --format "{{.Server.Version}}"')"

# A port already published by some other container makes `up -d` fail after
# the whole build and transfer. Our own container holding it is a redeploy.
#
# PORT is a fact about the NAS, so it is set in deploy.env and nowhere else.
PORT="${PORT:-3000}"
HOLDER="$(nas "docker ps --filter publish=$PORT --format '{{.Names}}'" | grep -vx "$CONTAINER" || true)"
[ -z "$HOLDER" ] || die "Port $PORT on the NAS is already published by $HOLDER. Set PORT in deploy/.env to a free one."
ok "port $PORT is free"

if nas 'docker compose version >/dev/null 2>&1'; then
    COMPOSE='docker compose'
elif nas 'command -v docker-compose >/dev/null'; then
    COMPOSE='docker-compose'
    warn "using the old docker-compose binary"
else
    die "No compose plugin on the NAS. Install docker-compose-plugin."
fi
ok "compose available as '$COMPOSE'"

# The reverse proxy's network, if one fronts this server. A proxy container
# reaches another by name only across a network they share, and compose puts
# this service on a network of its own. Joined here rather than by publishing a
# port for the proxy, so the route survives the NAS changing address. On that
# network the server answers as `masterphototools:3000` — the service name,
# lowercase, which compose registers as an alias on every network it joins.
if [ -n "${PROXY_NETWORK:-}" ]; then
    nas "docker network inspect '$PROXY_NETWORK' >/dev/null 2>&1" || {
        printf '\n  %sNo docker network %s.%s Networks on the NAS:\n\n' "$BOLD" "$PROXY_NETWORK" "$RESET"
        nas "docker network ls --format '    {{.Name}}'"
        die "Set PROXY_NETWORK in $CONFIG to the network the reverse proxy is on, from the list above."
    }
    ok "joins network $PROXY_NETWORK — the proxy reaches it as http://masterphototools:3000"
fi

# ---------------------------------------------------------------------------
# 2. The three directories, and who may write to them
# ---------------------------------------------------------------------------
#
# This is where an OMV deployment goes wrong, and it goes wrong silently: the
# container runs as a fixed uid, OMV shared folders are owned by whoever made
# them, and a mismatch means the tools come up, take a job, and fail on the
# first write — with a permission error nobody sees until they look for it.

step "Directories"

# The deployment's own folder — compose file, .env, and by default the ledger
# and the publishing folder beneath it. Checked now rather than at the mkdir in
# step 5: a folder made through the OMV web UI, or as root, is often not
# writable by the SSH user, and finding that out after a cross-build wastes the
# build.
if nas "test -d '$REMOTE_DIR'"; then
    nas "test -w '$REMOTE_DIR' || sudo -n true 2>/dev/null" \
        || die "$NAS_SSH cannot write to $REMOTE_DIR and has no passwordless sudo. On the NAS: sudo chown $(nas 'id -un') '$REMOTE_DIR'"
    ok "deployment   $REMOTE_DIR  (owned by $(nas "stat -c '%U:%G' '$REMOTE_DIR'"))"
else
    ok "deployment   $REMOTE_DIR  (will be created)"
fi

[ -n "${LIBRARY_PATH:-}" ] || {
    printf '\n  %sLIBRARY_PATH is not set.%s Shared folders on this NAS:\n\n' "$BOLD" "$RESET"
    nas 'ls -1d /srv/dev-disk-by-uuid-*/* /srv/*/  2>/dev/null | head -40' || true
    die "Set LIBRARY_PATH in $CONFIG to the photo library, from the list above."
}

: "${DATA_PATH:=$REMOTE_DIR/data}"
: "${PUBLISHING_PATH:=$REMOTE_DIR/publishing}"

nas "test -d '$LIBRARY_PATH'" || die "LIBRARY_PATH $LIBRARY_PATH does not exist on the NAS."
ok "library      $LIBRARY_PATH"
note "$(nas "df -h '$LIBRARY_PATH' | tail -1 | awk '{print \$4\" free of \"\$2}'")"

# The one mistake that deletes photographs. The server refuses this at startup
# too, but finding out here costs a second rather than a whole transfer.
case "$PUBLISHING_PATH" in
    "$LIBRARY_PATH"|"$LIBRARY_PATH"/*)
        [ "$PUBLISHING_PATH" = "$LIBRARY_PATH" ] && die \
            "PUBLISHING_PATH and LIBRARY_PATH are both $LIBRARY_PATH. Publishing empties that folder — this would delete the library."
        warn "publishing folder is inside the library"
        note "allowed, and only that folder is ever emptied — but keep it out of anything you sync"
        ;;
esac
case "$LIBRARY_PATH" in
    "$PUBLISHING_PATH"/*)
        die "The library $LIBRARY_PATH sits inside PUBLISHING_PATH $PUBLISHING_PATH. Everything under the publishing folder is published and then deleted."
        ;;
esac

ok "data         $DATA_PATH"
ok "publishing   $PUBLISHING_PATH"

# Whose uid owns the library decides what the container must run as. The image
# ships uid 10001; if the library belongs to somebody else, say so rather than
# quietly writing as the wrong user.
#
# The owner of the *photographs*, not of the folder: on OMV a shared folder is
# root's, and the files in it belong to whoever copied them in over SMB. The
# tools rewrite those files, and a replacement written by another uid is a file
# its owner can no longer edit from the Mac. A sample is enough — a library is
# one person's files. The folder's owner is the fallback for an empty library.
LIB_OWNER="$(nas "find '$LIBRARY_PATH' -type f ! -name '.*' 2>/dev/null | head -500 \
                  | xargs -r -d '\n' stat -c '%u:%g' | sort | uniq -c | sort -rn \
                  | awk 'NR==1 {print \$2}'")"
[ -n "$LIB_OWNER" ] || LIB_OWNER="$(nas "stat -c '%u:%g' '$LIBRARY_PATH'")"
LIB_OWNER_NAME="$(nas "getent passwd '${LIB_OWNER%%:*}' | cut -d: -f1")"
RUN_AS="${PHOTOTOOLS_UID:-}:${PHOTOTOOLS_GID:-}"
if [ "$RUN_AS" = ":" ]; then
    # Root in the container is root on every file in the library, and the
    # image drops privileges precisely so that nothing has to trust it.
    [ "${LIB_OWNER%%:*}" != 0 ] \
        || die "The photographs in $LIBRARY_PATH belong to root, and the container will not run as root. Set PHOTOTOOLS_UID and PHOTOTOOLS_GID in $CONFIG to the account that copies photographs in."
    RUN_AS="$LIB_OWNER"
    note "no PHOTOTOOLS_UID set, so the container will run as the photographs' owner, $LIB_OWNER (${LIB_OWNER_NAME:-no name})"
elif [ "$RUN_AS" != "$LIB_OWNER" ]; then
    warn "container runs as $RUN_AS; the photographs are owned by $LIB_OWNER"
fi
ok "runs as      $RUN_AS"

# Can that user actually create a file in the library's folders?
#
# **Rewriting a tag creates a file.** exiftool writes `NAME_exiftool_tmp` beside
# the original and renames it over the top, so a folder the container may read
# but not write fails every repair — and the folder, not the photograph, is what
# decides. A library whose top level is group-writable can easily hold subfolders
# that are not: a copy made as root leaves root:root 755 behind, and nothing
# about the library's own permissions says so.
#
# Sampled rather than exhaustive: a library is hundreds of thousands of files and
# this question has the same answer for a whole tree, almost always.
RUN_UID="${RUN_AS%%:*}"
RUN_GID="${RUN_AS#*:}"
UNWRITABLE="$(nas "find '$LIBRARY_PATH' -type d 2>/dev/null | head -200 \
    | xargs -r -d '\n' stat -c '%u %g %a %n' 2>/dev/null \
    | awk -v uid=$RUN_UID -v gid=$RUN_GID '
        {
            mode = \$3 + 0
            owner = int(mode / 100) % 10
            group = int(mode / 10) % 10
            other = mode % 10
            writable = 0
            if (\$1 == uid && owner % 4 >= 2) writable = 1
            if (\$2 == gid && group % 4 >= 2) writable = 1
            if (other % 4 >= 2) writable = 1
            if (!writable) { \$1=\$2=\$3=blank; sub(/^ +/, blank); print; exit }
        }'")"

if [ -n "$UNWRITABLE" ]; then
    warn "$RUN_AS cannot create files in $UNWRITABLE"
    note "the tools rewrite a tag by writing a temporary file beside the photograph,"
    note "so every repair in that folder fails. On the NAS, as root:"
    note "  chown -R ${LIB_OWNER_NAME:-$RUN_UID}:$RUN_GID '$LIBRARY_PATH'"
    note "  find '$LIBRARY_PATH' -type d -exec chmod 2775 {} +"
    note "  find '$LIBRARY_PATH' -type f -exec chmod 664 {} +"
    note "(an ACL may still permit it — this reads the mode bits only)"
else
    ok "library is writable by $RUN_AS"
fi

# ---------------------------------------------------------------------------
# 3. The environment the server will read, and the one the web UI is built with
# ---------------------------------------------------------------------------
#
# Nothing here is typed a second time. Every value the NAS needs already exists
# on this Mac, in the files the server and the web UI run with locally:
#
#   .env               the server's identity and credentials. Only the keys
#                      below are taken — ROOTS, the database, the port and the
#                      redirect URI in it describe this Mac, not the NAS.
#   frontend/web/.env  the Firebase web configuration, baked into the bundle.
#   deploy/.env        optional. Anything that must differ on the NAS; it wins.
#
# The host paths, the port and the uid come from deploy.env and from what step
# 2 found, and are written last so neither file can contradict them.

step "Configuration"

SHARED_KEYS='FIREBASE_PROJECT_ID|ALLOWED_UIDS|ADMIN_TOKEN|GOOGLE_OAUTH_CLIENT_ID|GOOGLE_OAUTH_CLIENT_SECRET|GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY|MAX_MEGAPIXELS|MAX_OUTPUT_BYTES|MAX_AGE_DAYS|OWNTRACKS_USER|OWNTRACKS_TOKEN|TIMELINE_OFFSET_MINUTES|RUST_LOG'
DEPLOY_OWNED='LIBRARY_PATH|DATA_PATH|PUBLISHING_PATH|PHOTOTOOLS_TAG|PHOTOTOOLS_USER|PORT'

[ -f .env ] || [ -f deploy/.env ] \
    || die "Neither .env nor deploy/.env exists. The server's Firebase and Google values live in one of them."

REMOTE_ENV="$(mktemp)"
chmod 600 "$REMOTE_ENV"
{
    echo "# Written by deploy/deploy.sh from .env and deploy/.env on the Mac — edit those, not this."
    # Leading blanks and an `export` are stripped before anything is matched.
    # A key anchored to the start of the line looks strict and is merely
    # brittle: one indented `ADMIN_TOKEN` was dropped in silence, the server
    # was deployed without a break-glass token, and everything that used it
    # answered 401 for a reason nothing on either machine mentioned.
    {
        if [ -f .env ]; then
            sed -E 's/^[[:space:]]+//; s/^export[[:space:]]+//' .env \
                | grep -E "^($SHARED_KEYS)=" || true
        fi
        if [ -f deploy/.env ]; then
            sed -E 's/^[[:space:]]+//; s/^export[[:space:]]+//' deploy/.env \
                | grep -vE "^[[:space:]]*(#|$)|^($DEPLOY_OWNED)=" || true
        fi
    } | awk -F= '{ if (!($1 in v)) order[++n] = $1; v[$1] = $0 }
                 END { for (i = 1; i <= n; i++) print v[order[i]] }'
    echo "LIBRARY_PATH=$LIBRARY_PATH"
    echo "DATA_PATH=$DATA_PATH"
    echo "PUBLISHING_PATH=$PUBLISHING_PATH"
    echo "PHOTOTOOLS_TAG=$TAG"
    echo "PHOTOTOOLS_USER=$RUN_AS"
    echo "PORT=$PORT"
} > "$REMOTE_ENV"

# A value as the server will see it: the last assignment, quotes removed.
server_value() { sed -n "s/^$1=//p" "$REMOTE_ENV" | tail -1 | sed -E 's/^"(.*)"$/\1/'; }

# The break-glass token is what the desktop authenticates its sync and handoff
# with (`known-gaps.md`), so its absence is worth a word rather than a silence.
if [ -z "$(server_value ADMIN_TOKEN)" ]; then
    warn "no ADMIN_TOKEN — the desktop cannot sync with this server until Firebase sign-in reaches it"
fi

for required in FIREBASE_PROJECT_ID ALLOWED_UIDS; do
    [ -n "$(server_value "$required")" ] \
        || die "$required is set in neither .env nor deploy/.env. Without it nobody can sign in."
done
ok "server: Firebase project $(server_value FIREBASE_PROJECT_ID), $(server_value ALLOWED_UIDS | tr ',' '\n' | grep -c .) allowed uid(s)"

OFFSET_MINUTES="$(server_value TIMELINE_OFFSET_MINUTES)"
if [ -n "$(server_value OWNTRACKS_USER)" ] && [ -n "$(server_value OWNTRACKS_TOKEN)" ]; then
    ok "server: accepts position reports from a phone as $(server_value OWNTRACKS_USER)"
    note "a day is counted at TIMELINE_OFFSET_MINUTES=${OFFSET_MINUTES:-0} minutes east of UTC — set it in deploy/.env if that is wrong"
else
    note "no OWNTRACKS_USER/OWNTRACKS_TOKEN, so the phone's position route refuses everything"
fi

if [ -n "$(server_value GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY)" ] && [ -n "$(server_value GOOGLE_OAUTH_CLIENT_ID)" ]; then
    ok "server: Google OAuth client and encryption key"
else
    warn "no Google OAuth client or encryption key — the server starts, and publishing cannot connect"
fi

# Deliberately not carried over from .env: the Mac's points at localhost, and
# Google would send the NAS's sign-in back to whichever machine the browser is
# on. It has to name the NAS, and be registered on the OAuth client.
if [ -z "$(server_value GOOGLE_OAUTH_REDIRECT_URI)" ]; then
    warn "GOOGLE_OAUTH_REDIRECT_URI is not set for the NAS — publishing cannot connect to Google until it is"
    note "put it in deploy/.env; it must be registered on the OAuth client exactly (MV-12.3)"
fi

# The web UI's Firebase configuration is compiled into the bundle, so it goes
# in as build arguments. .dockerignore keeps every .env out of the build
# context, and rightly — this file included — so without these the image ships
# a web UI whose sign-in screen lists four missing variables.
#
# Arguments rather than a build secret because none of the four is secret:
# each is in the JavaScript every browser downloads. And an argument, unlike a
# secret, invalidates the cached layer when it changes.
[ -f frontend/web/.env ] || die "No frontend/web/.env — the web UI would be built without Firebase, and nobody could sign in."
WEB_ARGS=()
for key in VITE_FIREBASE_API_KEY VITE_FIREBASE_AUTH_DOMAIN VITE_FIREBASE_PROJECT_ID VITE_FIREBASE_APP_ID; do
    value="$(set -a; . ./frontend/web/.env; printenv "$key" || true)"
    [ -n "$value" ] || die "$key is not set in frontend/web/.env."
    WEB_ARGS+=(--build-arg "$key=$value")
done

# Two projects is the failure that looks like everything working: sign-in
# succeeds against one, and the server refuses every token as issued by another.
WEB_PROJECT="$(set -a; . ./frontend/web/.env; printenv VITE_FIREBASE_PROJECT_ID)"
[ "$WEB_PROJECT" = "$(server_value FIREBASE_PROJECT_ID)" ] \
    || die "The web UI signs in to Firebase project $WEB_PROJECT, and the server accepts $(server_value FIREBASE_PROJECT_ID). They must be the same."
ok "web UI: Firebase project $WEB_PROJECT, from frontend/web/.env"

if $CHECK_ONLY; then
    step "Check only — nothing was built, sent or started"
    printf '  Deploy with: %s./deploy/deploy.sh%s\n\n' "$BOLD" "$RESET"
    exit 0
fi

# ---------------------------------------------------------------------------
# 4. Build
# ---------------------------------------------------------------------------

step "Build for $PLATFORM"

if [ -n "$(git status --porcelain)" ]; then
    warn "the working tree has uncommitted changes — they will be in the image"
fi

docker buildx inspect "$BUILDER" >/dev/null 2>&1 \
    || docker buildx create --name "$BUILDER" --driver docker-container >/dev/null
ok "buildx builder '$BUILDER'"

docker buildx build \
    --builder "$BUILDER" \
    --platform "$PLATFORM" \
    -f deploy/Dockerfile \
    "${WEB_ARGS[@]}" \
    -t "$IMAGE:$TAG" \
    --load \
    .

SIZE="$(docker image inspect "$IMAGE:$TAG" --format '{{.Size}}' | awk '{printf "%.0f MB", $1/1048576}')"
ok "$IMAGE:$TAG  ($SIZE uncompressed)"

# ---------------------------------------------------------------------------
# 5. Ship
# ---------------------------------------------------------------------------

step "Send to $NAS_SSH"

# Without sudo first, because the SSH user may well be root on OMV and `sudo`
# is not always installed on a box whose only account is root.
nas "mkdir -p '$REMOTE_DIR' '$DATA_PATH' '$PUBLISHING_PATH' 2>/dev/null \
     || sudo -n mkdir -p '$REMOTE_DIR' '$DATA_PATH' '$PUBLISHING_PATH'" \
    || die "Cannot create $REMOTE_DIR on the NAS. Make it by hand, or give $NAS_SSH write access to its parent."
ok "directories exist"

# Group-writable and setgid, the way every other folder in an OMV docker-data
# is. The chown below needs root, which an ordinary SSH account does not have;
# this is what still lets a container running as any member of `users` open
# the ledger and empty the publishing folder when it fails.
nas "chmod 2775 '$DATA_PATH' '$PUBLISHING_PATH' 2>/dev/null" || true

# Owned by whoever the container runs as, or the ledger cannot be opened and
# the publishing folder cannot be emptied. The library is left alone: it is the
# user's, and changing its ownership is not this script's business.
#
# Giving a folder to another uid needs root, so an ordinary SSH account fails
# here on every deploy. That is only a problem when the chmod above does not
# cover it — when the container's group is not the folders' group.
if ! nas "chown '$RUN_AS' '$DATA_PATH' '$PUBLISHING_PATH' 2>/dev/null \
          || sudo -n chown '$RUN_AS' '$DATA_PATH' '$PUBLISHING_PATH' 2>/dev/null"; then
    if [ "$(nas "stat -c '%g' '$DATA_PATH' '$PUBLISHING_PATH' | sort -u")" = "${RUN_AS#*:}" ]; then
        note "data and publishing stay owned by $NAS_SSH; the container's group, ${RUN_AS#*:}, writes to them"
    else
        warn "could not chown $DATA_PATH and $PUBLISHING_PATH to $RUN_AS — do it by hand if the server will not start"
    fi
fi

scp -q "${SSH_OPTS[@]}" deploy/docker-compose.yml "$NAS_SSH:$REMOTE_DIR/docker-compose.yml"
scp -q "${SSH_OPTS[@]}" "$REMOTE_ENV" "$NAS_SSH:$REMOTE_DIR/.env"
nas "chmod 600 '$REMOTE_DIR/.env'"

# An override rather than a line in docker-compose.yml: an external network
# must exist, and a deployment with no proxy has none to name. Compose merges
# docker-compose.override.yml on its own; removing it takes the service back
# off the proxy's network on the next deploy.
if [ -n "${PROXY_NETWORK:-}" ]; then
    nas "cat > '$REMOTE_DIR/docker-compose.override.yml'" <<EOF
# Written by deploy/deploy.sh because PROXY_NETWORK is set in deploy/deploy.env.
services:
  masterphototools:
    networks: [default, proxy]
networks:
  proxy:
    external: true
    name: $PROXY_NETWORK
EOF
else
    nas "rm -f '$REMOTE_DIR/docker-compose.override.yml'"
fi
ok "compose file and environment in $REMOTE_DIR"

# Piped rather than staged: a NAS is exactly the machine with no room for a
# 500 MB tar it only needs for ten seconds. gzip because it is everywhere.
printf '  sending image'
docker save "$IMAGE:$TAG" | gzip -1 | nas 'gunzip | docker load' | sed 's/^/\n  /'

# ---------------------------------------------------------------------------
# 6. Start, and confirm it actually answers
# ---------------------------------------------------------------------------

step "Start"

# `up -d` alone reports success for a container that starts and immediately
# exits on a bad configuration, which is the whole failure mode worth catching.
#
# --no-build because the compose file's build context is `..`, which on the NAS
# is whatever folder holds REMOTE_DIR — a docker-data folder full of other
# services' data. The image was loaded a moment ago; if it somehow is not
# there, failing is right and tarring up the neighbours is not.
nas "cd '$REMOTE_DIR' && $COMPOSE up -d --no-build --remove-orphans"

HOST="${NAS_SSH#*@}"

# Asked from the NAS itself, so on the host port compose published, not the
# container's 3000.
printf '  waiting for /api/health'
for _ in $(seq 1 30); do
    if HEALTH="$(nas "curl -sf --max-time 2 http://127.0.0.1:$PORT/api/health" 2>/dev/null)"; then
        printf '\n'
        ok "$HEALTH"
        step "Deployed"
        printf '  Web UI:  %shttp://%s:%s/%s\n' "$BOLD" "$HOST" "$PORT" "$RESET"
        printf '  Logs:    ssh %s "cd %s && %s logs -f"\n\n' "$NAS_SSH" "$REMOTE_DIR" "$COMPOSE"
        exit 0
    fi
    printf '.'
    sleep 2
done

printf '\n'
warn "no answer from /api/health after a minute"
note "the usual cause is a configuration the server refused to start on — it says which:"
printf '\n'
nas "cd '$REMOTE_DIR' && $COMPOSE logs --tail 20"
exit 1
