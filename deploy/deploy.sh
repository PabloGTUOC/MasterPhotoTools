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
IMAGE="phototools-server"
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

# ConnectTimeout because the failure this guards against is a NAS that is
# asleep or renamed, and ssh's own default is to sit there for two minutes
# before saying so.
nas() { ssh -o BatchMode=yes -o ConnectTimeout=10 "$NAS_SSH" "$@"; }

# ---------------------------------------------------------------------------
# 1. Reach the NAS, and find out what it is
# ---------------------------------------------------------------------------

step "The NAS"

nas true 2>/dev/null || die "Cannot ssh to $NAS_SSH without a password. Check the host, or run ssh-copy-id."
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
ok "docker $(nas 'docker version --format "{{.Server.Version}}"')"

if nas 'docker compose version >/dev/null 2>&1'; then
    COMPOSE='docker compose'
elif nas 'command -v docker-compose >/dev/null'; then
    COMPOSE='docker-compose'
    warn "using the old docker-compose binary"
else
    die "No compose plugin on the NAS. Install docker-compose-plugin."
fi
ok "compose available as '$COMPOSE'"

# ---------------------------------------------------------------------------
# 2. The three directories, and who may write to them
# ---------------------------------------------------------------------------
#
# This is where an OMV deployment goes wrong, and it goes wrong silently: the
# container runs as a fixed uid, OMV shared folders are owned by whoever made
# them, and a mismatch means the tools come up, take a job, and fail on the
# first write — with a permission error nobody sees until they look for it.

step "Directories"

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
LIB_OWNER="$(nas "stat -c '%u:%g' '$LIBRARY_PATH'")"
RUN_AS="${PHOTOTOOLS_UID:-}:${PHOTOTOOLS_GID:-}"
if [ "$RUN_AS" = ":" ]; then
    RUN_AS="$LIB_OWNER"
    note "no PHOTOTOOLS_UID set, so the container will run as the library's owner, $LIB_OWNER"
elif [ "$RUN_AS" != "$LIB_OWNER" ]; then
    warn "container runs as $RUN_AS; the library is owned by $LIB_OWNER"
    note "the tools rewrite metadata in place — make sure $RUN_AS can write there"
fi
ok "runs as      $RUN_AS"

if $CHECK_ONLY; then
    step "Check only — nothing was built, sent or started"
    printf '  Deploy with: %s./deploy/deploy.sh%s\n\n' "$BOLD" "$RESET"
    exit 0
fi

# ---------------------------------------------------------------------------
# 3. The environment the server will read
# ---------------------------------------------------------------------------

step "Server configuration"

[ -f deploy/.env ] || die "No deploy/.env. Copy deploy/.env.example and fill it in — it holds the Firebase and Google values."

for required in FIREBASE_PROJECT_ID ALLOWED_UIDS; do
    grep -qE "^${required}=.+" deploy/.env \
        || die "$required is not set in deploy/.env. ALLOWED_UIDS empty means nobody can sign in."
done
ok "deploy/.env has the required values"

# Assembled here and sent with the deployment: the host paths belong to this
# machine's view of the NAS, and the secrets belong to deploy/.env. Neither is
# committed.
REMOTE_ENV="$(mktemp)"
trap 'rm -f "$REMOTE_ENV"' EXIT
{
    echo "# Written by deploy/deploy.sh — edit deploy/.env on the Mac, not this."
    grep -vE '^(LIBRARY_PATH|DATA_PATH|PUBLISHING_PATH|PHOTOTOOLS_TAG)=' deploy/.env
    echo "LIBRARY_PATH=$LIBRARY_PATH"
    echo "DATA_PATH=$DATA_PATH"
    echo "PUBLISHING_PATH=$PUBLISHING_PATH"
    echo "PHOTOTOOLS_TAG=$TAG"
    echo "PHOTOTOOLS_USER=$RUN_AS"
} > "$REMOTE_ENV"

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

# Owned by whoever the container runs as, or the ledger cannot be opened and
# the publishing folder cannot be emptied. The library is left alone: it is the
# user's, and changing its ownership is not this script's business.
nas "chown '$RUN_AS' '$DATA_PATH' '$PUBLISHING_PATH' 2>/dev/null \
     || sudo -n chown '$RUN_AS' '$DATA_PATH' '$PUBLISHING_PATH' 2>/dev/null" \
    || warn "could not chown $DATA_PATH and $PUBLISHING_PATH to $RUN_AS — do it by hand if the server will not start"

scp -q deploy/docker-compose.yml "$NAS_SSH:$REMOTE_DIR/docker-compose.yml"
scp -q "$REMOTE_ENV" "$NAS_SSH:$REMOTE_DIR/.env"
nas "chmod 600 '$REMOTE_DIR/.env'"
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
nas "cd '$REMOTE_DIR' && $COMPOSE up -d --remove-orphans"

PORT="$(grep -E '^PORT=' deploy/.env | cut -d= -f2- || true)"
PORT="${PORT:-3000}"
HOST="${NAS_SSH#*@}"

printf '  waiting for /api/health'
for _ in $(seq 1 30); do
    if HEALTH="$(nas "curl -sf --max-time 2 http://127.0.0.1:3000/api/health" 2>/dev/null)"; then
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
