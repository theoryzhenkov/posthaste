#!/usr/bin/env bash
set -euo pipefail

# Per-channel policy table.
#
# The single source of truth for what a channel means: identity, updater
# manifest, devtools, signing. Jobs call this with a channel and materialize the
# result into GITHUB_ENV. Adding a channel = one new case, not re-threading
# booleans through every job.
#
# Usage:
#   channel-policy.sh <channel>

channel="${1:?usage: channel-policy.sh <channel>}"

case "$channel" in
  nightly)
    identifier="com.posthaste.mail.nightly"
    product_name="PosthasteNightly"
    updater_manifest="latest.json"
    updater_endpoint="https://github.com/theoryzhenkov/posthaste/releases/download/nightly/latest.json"
    include_devtools="true"
    enforce_macos_signing="false"
    is_stable="false"
    authority_runtime_server_name="PosthasteAuthorityRuntimeServerNightly"
    cli_name="PosthasteCTLNightly"
    web_name="PosthasteWebNightly"
    authority_server_name="PosthasteAuthorityServerNightly"
    runtime_name="PosthasteRuntimeNightly"
    wizard_name="PosthasteWizardNightly"
    icon_dir="icons-nightly"
    ;;
  stable)
    identifier="com.posthaste.mail"
    product_name="Posthaste"
    updater_manifest="latest-stable.json"
    updater_endpoint="https://github.com/theoryzhenkov/posthaste/releases/download/stable/latest-stable.json"
    include_devtools="false"
    enforce_macos_signing="true"
    is_stable="true"
    authority_runtime_server_name="PosthasteAuthorityRuntimeServer"
    cli_name="PosthasteCTL"
    web_name="PosthasteWeb"
    authority_server_name="PosthasteAuthorityServer"
    runtime_name="PosthasteRuntime"
    wizard_name="PosthasteWizard"
    icon_dir="icons"
    ;;
  *)
    echo "error: unknown channel '$channel' (expected: nightly|stable)" >&2
    exit 1
    ;;
esac

cat <<EOF
POSTHASTE_IDENTIFIER=$identifier
POSTHASTE_PRODUCT_NAME=$product_name
POSTHASTE_UPDATER_MANIFEST=$updater_manifest
POSTHASTE_UPDATER_ENDPOINT=$updater_endpoint
POSTHASTE_RELEASE_CHANNEL=$channel
POSTHASTE_INCLUDE_DEVTOOLS=$include_devtools
POSTHASTE_ENFORCE_MACOS_SIGNING=$enforce_macos_signing
POSTHASTE_IS_STABLE=$is_stable
POSTHASTE_AUTHORITY_RUNTIME_SERVER_NAME=$authority_runtime_server_name
POSTHASTE_CLI_NAME=$cli_name
POSTHASTE_WEB_NAME=$web_name
POSTHASTE_AUTHORITY_SERVER_NAME=$authority_server_name
POSTHASTE_RUNTIME_NAME=$runtime_name
POSTHASTE_WIZARD_NAME=$wizard_name
POSTHASTE_ICON_DIR=$icon_dir
POSTHASTE_RUN_ARTIFACT_SMOKE=true
EOF
