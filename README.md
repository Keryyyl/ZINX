# ZINX

🇵🇱 [Wersja polska](README.pl.md)

ZINX is a set of personal desktop apps for macOS that work together. Each app has its own local SQLite database, and nothing leaves the machine except what the features need, such as fetching mail or weather. The AI features run on local models: a language model through [Ollama](https://ollama.com) and speech recognition through [whisper.cpp](https://github.com/ggerganov/whisper.cpp).

Each app is a separate project with its own `CLAUDE.md`, which describes its architecture and rules.

## Apps

| Project | What it does |
|---|---|
| **[ZIM](ZIM/)** | Mail client. Syncs mailboxes, sorts messages with a local AI model, sends mail and manages an outgoing queue. Tauri 2 port of the earlier Electron app (`ZIM-OG`, not included here). |
| **[CTHX](CTHX/)** | Planner: month calendar, tasks and events for each day, habit tracker and a kanban board. Supports repeating entries and birthdays from contacts. |
| **[PFMX](PFMX/)** | Personal finance: accounts, a budget in envelopes, recurring payments and subscriptions, cash tracking, investments. Imports bank history, categorises expenses with a local model, and attaches receipts to transactions. |
| **[EINPX](EINPX/)** | Dashboard: one window showing mail, balances and spending from PFMX, the day plan from CTHX, plus weather and quick notes. |
| **[Klaudiusz](Klaudiusz/)** | Polish voice assistant in the macOS menu bar. Listens for its name, reads data from the other apps, adds entries after a spoken confirmation, sends reminders and has keyboard shortcuts. |
| **Exporter** | A private Python script that downloads bank statement history to CSV. It is not part of this repository because it works with personal login data. |

## How they fit together

- Each app keeps its data in its own SQLite database under `~/Library/Application Support/`.
- Apps that read other apps' data (EINPX, Klaudiusz) open those databases read-only. Writes go through the owning app, and only after the user confirms them.
- ZIM, CTHX, PFMX and EINPX share one visual style, so they look like parts of the same system.

## Tech stack

- **Tauri 2** (Rust backend, WebView frontend) for ZIM, CTHX, PFMX, EINPX and Klaudiusz.
- **React, Vite and TypeScript** for the interfaces.
- **SQLite** (via `rusqlite`) for storage.
- **Ollama** and **whisper.cpp** for local AI features.

## Running an app

Each project runs separately, from its own folder:

```bash
pnpm install
pnpm tauri dev
```

Core tests (Rust):

```bash
cd <project>/src-tauri && cargo test
```

Requirements: macOS, Node.js, Rust (via rustup) and pnpm. Klaudiusz also needs Ollama and whisper.cpp.

## Privacy

- The repository contains no passwords, API keys, bank account numbers or private databases. Test data uses example values.
- Mail passwords are kept in the system keychain, not in files.
- The bank export script, the `ZIM-OG` repository and personal data files are excluded by `.gitignore`.

## Status

These are personal projects built and used day to day. They are shared as a portfolio, not as packaged products. Expect rough edges.
