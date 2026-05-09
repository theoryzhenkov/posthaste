{
  pkgs,
  system,
  theor-project,
}:

let
  playwrightBrowsers = pkks: pkks.playwright-driver.browsers;
  linuxBrowserRuntime = with pkgs; [
    glib
    nspr
    nss
    dbus
    atk
    at-spi2-atk
    cups
    expat
    libxkbcommon
    pango
    cairo
    alsa-lib
    udev
    mesa
    libx11
    libxcomposite
    libxdamage
    libxext
    libxfixes
    libxrandr
    libxcb
  ];
in
{
  packages = [
    pkgs.gnupg
    pkgs.rustc
    pkgs.cargo
    pkgs.rustfmt
    pkgs.rust-analyzer
    pkgs.pkg-config
    pkgs.nodejs_22
    pkgs.bun
    pkgs.playwright-driver
    (playwrightBrowsers pkgs)
    pkgs.stalwart
    pkgs.tmux
    pkgs.overmind
    pkgs.python3
    pkgs.python3Packages.mkdocs-material
  ]
  ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux (
    [
      pkgs.webkitgtk_4_1
      pkgs.libsoup_3
      pkgs.gtk3
      pkgs.glib-networking
      pkgs.openssl
      pkgs.libayatana-appindicator
    ]
    ++ linuxBrowserRuntime
  );

  shellHook = ''
    export POSTHASTE_OAUTH_SECRETS_FILE="$FLAKE_ROOT/secrets/oauth.yaml"
    export GIO_EXTRA_MODULES="${pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux "${pkgs.glib-networking}/lib/gio/modules"}"
    export PLAYWRIGHT_BROWSERS_PATH="${playwrightBrowsers pkgs}"
    export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
    export PLAYWRIGHT_NODEJS_PATH="${pkgs.nodejs_22}/bin/node"
    export POSTHASTE_PLAYWRIGHT_CLI="${pkgs.playwright-driver}/cli.js"
    export RUST_SRC_PATH="${pkgs.rustPlatform.rustLibSrc}"

    posthaste_export_sops_secret() {
      local secret_path="$1"
      local env_name="$2"
      local secret_value

      if [ ! -f "$POSTHASTE_OAUTH_SECRETS_FILE" ] || [ ! -f "$SOPS_AGE_KEY_FILE" ]; then
        return 0
      fi

      secret_value="$(sops --decrypt --extract "$secret_path" "$POSTHASTE_OAUTH_SECRETS_FILE" 2>/dev/null || true)"
      if [ -n "$secret_value" ] && [ "$secret_value" != "null" ]; then
        export "$env_name=$secret_value"
      fi
    }

    posthaste_export_sops_secret '["google_oauth_client_secret"]' VITE_GOOGLE_OAUTH_CLIENT_SECRET
    posthaste_export_sops_secret '["microsoft_oauth_client_secret"]' VITE_MICROSOFT_OAUTH_CLIENT_SECRET
    unset -f posthaste_export_sops_secret
  '';
}
