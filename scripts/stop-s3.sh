#!/usr/bin/env bash
# Stop and remove the sample S3 server (its data goes with it).
set -euo pipefail
docker rm -f cosmonic-demo-s3 >/dev/null 2>&1 && echo "Stopped cosmonic-demo-s3" || echo "cosmonic-demo-s3 was not running"
