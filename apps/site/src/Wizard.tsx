import type { HomeContent } from './content/types'
import { CopyableCommand } from './CopyableCommand'
import { FooterSection, InstallHeader } from './SiteChrome'

const BOOTSTRAP =
  'curl -fsSL https://posthaste.theor.net/install_wizard.sh | sh'

/**
 * Provision a backend node: fetch + verify + install `posthaste_backend`,
 * provision TLS, register a service, and print a one-line join string.
 *
 * Flags mirror `crates/posthaste-wizard/src/main.rs`; run
 * `posthaste-wizard --help` for the canonical surface.
 */
const BACKEND_CMD = [
  'posthaste-wizard install --role backend --tls --host backend.lan \\',
  '  --bind 0.0.0.0:3002 --link-token <secret> \\',
  '  --config-root ~/.config/posthaste --state-root ~/.local/share/posthaste',
].join('\n')

/**
 * Join a runtime node to the backend: the join string wires URL + token + CA,
 * so the second machine is one command with no manual copying.
 */
const RUNTIME_CMD = [
  'posthaste-wizard install --role runtime --bind 0.0.0.0:3001 \\',
  '  --config-root ~/.config/posthaste --state-root ~/.local/share/posthaste \\',
  '  --join <join-string>',
].join('\n')

export function Wizard({ footer }: { footer: HomeContent['footer'] }) {
  return (
    <main className="site-shell">
      <InstallHeader active="wizard" />

      <section className="wizard-hero" aria-labelledby="wizard-title">
        <h1 id="wizard-title">Wizard</h1>
        <p className="wizard-tagline">
          Run Posthaste&apos;s backend on your own server — distributed,
          TLS-ready, and checksum-verified.
        </p>
        <CopyableCommand command={BOOTSTRAP} />
        <p className="wizard-hero-note">
          Installs <code>posthaste-wizard</code> to <code>~/.local/bin</code>.
        </p>
      </section>

      <section className="wizard-steps" aria-labelledby="wizard-steps-title">
        <h2 id="wizard-steps-title">Set up a backend and join a runtime</h2>

        <ol className="wizard-step-list">
          <li className="wizard-step">
            <h3>
              <span className="wizard-step-num" aria-hidden="true">
                1
              </span>
              Provision the backend
            </h3>
            <p className="wizard-step-note">
              On your server. Fetches + verifies + installs{' '}
              <code>posthaste_backend</code>, provisions TLS, registers a
              service, and prints a <strong>join string</strong>.
            </p>
            <CopyableCommand command={BACKEND_CMD} />
          </li>
          <li className="wizard-step">
            <h3>
              <span className="wizard-step-num" aria-hidden="true">
                2
              </span>
              Join a runtime
            </h3>
            <p className="wizard-step-note">
              On a second machine. The join string wires URL + token + CA — no
              manual copying.
            </p>
            <CopyableCommand command={RUNTIME_CMD} />
          </li>
        </ol>

        <p className="wizard-help">
          Every step is checksum-verified and fails closed. Run{' '}
          <code>posthaste-wizard --help</code> for the full flag reference, or
          use <code>--role daemon</code> for the all-in-one on a single machine.
        </p>
      </section>

      <FooterSection content={footer} />
    </main>
  )
}
