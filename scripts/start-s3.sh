#!/usr/bin/env bash
# Start a throwaway S3-compatible server (SeaweedFS) for this sample, on
# 127.0.0.1 only, with demo credentials and the bucket the app uses.
# Usage: scripts/start-s3.sh [port]
set -euo pipefail
PORT="${1:-9000}"
NAME="cosmonic-demo-s3"
IMAGE="chrislusf/seaweedfs:4.54"
BUCKET="cosmonic-notes"
# Demo credentials for a loopback-only throwaway server. Change them for
# anything that lives longer than a demo.
ACCESS_KEY="${S3_ACCESS_KEY:-cosmonic-demo}"
SECRET_KEY="${S3_SECRET_KEY:-cosmonic-demo-secret}"

command -v docker >/dev/null || { echo "docker is required (Docker Desktop, OrbStack, colima...)" >&2; exit 1; }

if docker ps --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "$NAME is already running at $(docker port "$NAME" 8333/tcp | head -1)"
  exit 0
fi
if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then
  echo "Something is already listening on 127.0.0.1:$PORT; pass another port." >&2
  exit 1
fi

CONF_DIR="$(mktemp -d)"
cat > "$CONF_DIR/s3.json" <<JSON
{"identities":[{"name":"cosmonic-demo","credentials":[{"accessKey":"$ACCESS_KEY","secretKey":"$SECRET_KEY"}],"actions":["Admin","Read","Write","List","Tagging"]}]}
JSON

docker rm -f "$NAME" >/dev/null 2>&1 || true
# Loopback only. Small volumes so it fits on a laptop with little free disk.
docker run -d --name "$NAME" -p "127.0.0.1:$PORT:8333" \
  -v "$CONF_DIR/s3.json:/etc/seaweedfs/s3.json:ro" \
  "$IMAGE" server -s3 -s3.config=/etc/seaweedfs/s3.json \
  -volume.max=16 -volume.minFreeSpace=1GiB -master.volumeSizeLimitMB=32 >/dev/null

echo "Waiting for SeaweedFS..."
for _ in $(seq 1 60); do
  if docker exec "$NAME" sh -c "echo 's3.bucket.create -name $BUCKET' | weed shell -master=localhost:9333" 2>/dev/null \
      | grep -q -E "created bucket|already exists"; then
    cat <<OUT
S3 is up: http://127.0.0.1:$PORT  (bucket: $BUCKET)

For the S3 binding named 'assets' in Cosmonic Desktop:
  endpoint          http://127.0.0.1:$PORT   (turn on "Allow plain HTTP")
  region            us-east-1
  access key ID     $ACCESS_KEY
  secret access key $SECRET_KEY
Store the two keys as secret refs; the binding names the refs, never the values.
OUT
    exit 0
  fi
  sleep 2
done
echo "SeaweedFS did not come up; see: docker logs $NAME" >&2
exit 1
