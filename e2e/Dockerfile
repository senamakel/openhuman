##
# DEPRECATED: E2E tests now use the shared CI image from GHCR.
#
# The CI image (.github/Dockerfile) includes all E2E dependencies
# (xvfb, dbus, tauri-driver, webkit2gtk-driver).
#
# Usage:
#   docker compose -f e2e/docker-compose.yml run --rm e2e
#
# To build the CI image locally (if you can't pull from GHCR):
#   docker build -t openhuman-ci -f .github/Dockerfile .
#   IMAGE=openhuman-ci docker compose -f e2e/docker-compose.yml run --rm e2e
#
FROM ghcr.io/tinyhumansai/openhuman_ci:latest

WORKDIR /app

ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["yarn", "workspace", "openhuman-app", "test:e2e:all"]
