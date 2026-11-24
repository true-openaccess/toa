#!/usr/bin/env bash
# The network test of toa (docs/lines/tools.md 9.5): one object fetched by name from a local
# git server, with the system git and with the built-in git, which must get the same files.
#   - https with a self-signed certificate, trusted through `http.sslCAInfo` in the user's git
#     configuration, through a proxy set by `http.proxy` there; without the certificate both
#     must fail;
#   - ssh to a local sshd, with the key in ssh-agent.
# Usage: fetch.sh PATH-TO-TOA [https|ssh|all]. Needs git, python3, openssl, and for ssh an
# sshd and ssh-agent. Meant for a CI machine: it starts servers and, for ssh, writes
# ~/.ssh/config.
set -euo pipefail

TOA=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
WHAT=${2:-all}
HERE=$(cd "$(dirname "$0")" && pwd)
W=$(mktemp -d)
trap 'kill $(jobs -p) 2>/dev/null || true' EXIT
export GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=T GIT_AUTHOR_EMAIL=t@example.org GIT_COMMITTER_NAME=T GIT_COMMITTER_EMAIL=t@example.org

fail() { echo "FAIL: $*" >&2; exit 1; }

# a library with one object, served from a bare repository
mkdir -p "$W/work/doc/a/doc_draft-a" "$W/srv"
git -C "$W/work" init -q -b main
printf '* -text\n' > "$W/work/.gitattributes"
printf 'type: doc\nlicense: CC0-1.0\ntitle: A document fetched over the network\n' > "$W/work/doc/a/doc_draft-a/toa.yaml"
printf 'hello\n' > "$W/work/doc/a/doc_draft-a/README.md"
"$TOA" --git=system seal "$W/work"
NAME=$(ls "$W/work/doc/a" | grep '^doc_[0-9a-f]*$')
git -C "$W/work" add -A && git -C "$W/work" commit -qm one
git clone -q --bare "$W/work" "$W/srv/lib.git"
git -C "$W/srv/lib.git" config uploadpack.allowAnySHA1InWant true
echo "object $NAME"

# fetch NAME from ADDRESS with each git; the files must be the same
fetch_both() {
  local label=$1 address=$2
  printf '{"toa": "source/1"}\n{"object": "%s", "sources": [{"git": "%s", "path": "doc/a/%s"}]}\n' \
    "$NAME" "$address" "$NAME" > "$W/workspace-$label.source.jsonl"
  for g in system builtin; do
    TOA_CACHE="$W/cache-$label-$g" "$TOA" --git=$g fetch "$NAME" --workspace "$W/workspace-$label.source.jsonl" \
      --into "$W/out-$label-$g" || fail "$label: the $g git did not fetch"
    test -f "$W/out-$label-$g/$NAME/README.md" || fail "$label: the $g git wrote nothing"
  done
  diff -r "$W/out-$label-system" "$W/out-$label-builtin" || fail "$label: the two gits fetched different files"
  echo "ok $label"
}

if [ "$WHAT" = https ] || [ "$WHAT" = all ]; then
  # a CA of one's own and a server certificate it signed, as on a company network. (A server
  # certificate that is itself a self-signed CA certificate is accepted by git, but refused by
  # the built-in git's TLS library: tools.md 9.4.)
  export MSYS2_ARG_CONV_EXCL=/CN=
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$W/ca.key" -out "$W/ca.pem" -days 1 -subj /CN=toa-test-ca 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -keyout "$W/key.pem" -out "$W/server.csr" -subj /CN=localhost 2>/dev/null
  printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n' > "$W/ext.cnf"
  openssl x509 -req -in "$W/server.csr" -CA "$W/ca.pem" -CAkey "$W/ca.key" -CAcreateserial -out "$W/cert.pem" \
    -days 1 -extfile "$W/ext.cnf" 2>/dev/null
  python3 "$HERE/serve.py" git --root "$W/srv" --cert "$W/cert.pem" --key "$W/key.pem" --port 8443 &
  python3 "$HERE/serve.py" proxy --port 3128 --log "$W/proxy.log" &
  for _ in $(seq 50); do (exec 3<>/dev/tcp/127.0.0.1/8443) 2>/dev/null && (exec 3<>/dev/tcp/127.0.0.1/3128) 2>/dev/null && break; sleep 0.2; done

  # the user's git configuration: the certificate and the proxy, and nothing in the environment
  export HOME="$W/home"
  mkdir -p "$HOME"
  unset HTTPS_PROXY https_proxy HTTP_PROXY http_proxy ALL_PROXY all_proxy NO_PROXY no_proxy
  git config --global http.sslCAInfo "$W/ca.pem"
  git config --global http.proxy http://127.0.0.1:3128
  # Git for Windows: its default TLS (schannel) asks for the revocation list of the CA, which a
  # test CA has not; use its OpenSSL, which reads http.sslCAInfo as git on Linux does
  case "$(uname -s)" in MINGW* | MSYS*) git config --global http.sslBackend openssl ;; esac
  before=$(wc -l < "$W/proxy.log")
  fetch_both https "https://localhost:8443/lib.git"
  after=$(wc -l < "$W/proxy.log")
  [ $((after - before)) -ge 2 ] || fail "https: the proxy saw $((after - before)) connection(s), not one per git"

  # without the certificate: refused by both, exit code 4 (not fetched)
  git config --global --unset http.sslCAInfo
  for g in system builtin; do
    set +e
    TOA_CACHE="$W/cache-untrusted-$g" "$TOA" --git=$g fetch "$NAME" --workspace "$W/workspace-https.source.jsonl" \
      --into "$W/out-untrusted-$g" 2>"$W/untrusted-$g.txt"
    code=$?
    set -e
    [ $code = 4 ] || { cat "$W/untrusted-$g.txt"; fail "an untrusted certificate: the $g git gave exit code $code"; }
  done
  echo "ok https without the certificate: refused by both"
fi

if [ "$WHAT" = ssh ] || [ "$WHAT" = all ]; then
  # ssh reads the configuration of the user's home in the password database, not of $HOME,
  # which the https part changed
  SSH_HOME=$(getent passwd "$(id -un)" | cut -d: -f6)
  mkdir -p "$W/ssh" "$SSH_HOME/.ssh"
  ssh-keygen -q -t ed25519 -N '' -f "$W/ssh/host"
  ssh-keygen -q -t ed25519 -N '' -f "$W/ssh/user"
  cp "$W/ssh/user.pub" "$W/ssh/authorized_keys"
  cat > "$W/ssh/sshd_config" <<EOF
Port 2222
ListenAddress 127.0.0.1
HostKey $W/ssh/host
AuthorizedKeysFile $W/ssh/authorized_keys
PasswordAuthentication no
StrictModes no
UsePAM no
PidFile $W/ssh/sshd.pid
EOF
  "$(command -v sshd || echo /usr/sbin/sshd)" -f "$W/ssh/sshd_config" -E "$W/ssh/sshd.log"
  echo "[localhost]:2222 $(cut -d' ' -f1,2 "$W/ssh/host.pub")" > "$W/ssh/known_hosts"
  cat >> "$SSH_HOME/.ssh/config" <<EOF
Host localhost
  Port 2222
  User $(id -un)
  IdentitiesOnly no
  UserKnownHostsFile $W/ssh/known_hosts
  StrictHostKeyChecking yes
  BatchMode yes
EOF
  eval "$(ssh-agent -s)" >/dev/null
  ssh-add -q "$W/ssh/user"
  fetch_both ssh "ssh://localhost$W/srv/lib.git"
  ssh-agent -k >/dev/null
fi
echo "all fetched alike"
