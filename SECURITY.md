# Security

## Reporting a problem

Please report vulnerabilities privately through GitHub: open the repository's Security tab and choose **Report a vulnerability**. Do not open a public issue for a security problem.

Include what you found, the steps to reproduce it, and the version of the app.

## Supported versions

Only the latest release gets fixes.

## What the app handles

- **API key.** The search API key you enter is stored as plain text in `state.json` under `~/Library/Application Support/com.8bittitan.as-gui/`. The file is created with mode 0600 and its folder with mode 0700. Use a search API key with the `search`, `settings` and `logs` ACLs, never an admin key.
- **Network.** Requests go only to `https://<app id>.algolia.net` and `https://<app id>-dsn.algolia.net`, over TLS with certificate checks. The app validates the Application ID (ten letters or digits) before it puts it in a hostname.
- **Selected text.** Ask about selection reads the selection by sending ⌘C to the front app and restoring your clipboard. It sends the text to the agent only when you ask, skips a clipboard that another app marked as secret, and never logs the text.
- **Logs.** Logs record request URLs, event types and errors. They do not hold your key or message text.
- **Releases.** Release builds are signed ad hoc and are not notarized. Each release lists a SHA-256 checksum for the disk image.

## Out of scope

Anything that needs control of your Mac account (for example reading the state file as you), and weaknesses in Algolia's own services. Report those to Algolia.
