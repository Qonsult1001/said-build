# How to download and install `said`

> Goal: get the `said` command working on your computer, starting from nothing. When
> you're done, `said --version` works in your terminal and you're ready for the
> [tutorial](cli/tutorial-your-first-brain.md).

`said` is a single self-contained program — no dependencies, nothing to configure. It works offline.
There are three ways to install it, easiest first.

## Option A — one-line install (recommended)

**Linux / macOS** — paste into a terminal:

    curl -fsSL https://github.com/Qonsult1001/said-build/releases/latest/download/install.sh | sh

**Windows** — paste into PowerShell:

    irm https://github.com/Qonsult1001/said-build/releases/latest/download/install.ps1 | iex

This downloads `said` + `said-mcp` for your OS, installs them, and adds them to your PATH. Reopen your
terminal and run `said --version`. Done — skip to the [tutorial](cli/tutorial-your-first-brain.md).

## Option B — native installer (double-click)

Download and run the installer for your OS from the
[releases page](https://github.com/Qonsult1001/said-build/releases/latest):

- **Windows** → `said-setup-<version>-x64.exe` — run it; it installs `said` + `said-mcp` and adds them
  to your PATH. Uninstall from *Add or Remove Programs*.
- **macOS** (Apple Silicon) → `said-<version>-arm64.pkg` — open it; installs into `/usr/local/bin`.
  (Unsigned for now: if macOS blocks it, right-click the `.pkg` → **Open**.)
- **Linux** (Debian/Ubuntu) → `said_<version>_amd64.deb` — install with
  `sudo apt install ./said_<version>_amd64.deb` (installs to `/usr/bin`).

## Option C — download the zip manually (no installer)

The most manual path — download, unzip, add to PATH yourself. Use this if you want to control exactly
where the binaries live.

## Step 1 — Download the right file for your computer

Go to the releases page: **https://github.com/Qonsult1001/said-build/releases/latest**

Download the **`brain`** build that matches your machine:

- **Windows** → `said-brain-windows-x64.zip`
- **macOS** (Apple Silicon — M1/M2/M3/M4) → `said-brain-macos-arm64.zip`
- **Linux** (64-bit Intel/AMD) → `said-brain-linux-x64.zip`

> The `brain` build is the portable personal-memory version this guide covers. (The
> `coding` / `full` builds add code and document features you don't need for memory use.)

## Step 2 — Unzip it

The zip contains the `said` program (a single file).

- **Windows** → right-click the `.zip` → **Extract All…** → pick a folder you'll remember,
  e.g. `C:\said`.
- **macOS** → double-click the `.zip` in Finder; it unzips next to itself.
- **Linux** → in a terminal:

      unzip said-brain-linux-x64.zip -d ~/said

## Step 3 — Make `said` runnable from anywhere

You want to type `said` in any folder. Pick the path for your OS:

- **Windows** → add the folder you extracted to (e.g. `C:\said`) to your **PATH**:
  Start menu → "Edit the system environment variables" → **Environment Variables** → under
  *User variables* select **Path** → **Edit** → **New** → paste `C:\said` → OK. Open a
  **new** terminal afterward.
- **macOS / Linux** → move the binary onto your PATH and mark it executable:

      chmod +x ~/said/said
      sudo mv ~/said/said /usr/local/bin/said

  - **If you can't use `sudo`** → keep it in `~/said` and run it as `~/said/said` instead
    of `said`, or add `export PATH="$HOME/said:$PATH"` to your `~/.bashrc` / `~/.zshrc`.

### macOS only — clear the "unidentified developer" block

The first time you run a downloaded binary, macOS may refuse it. If you see that:

    xattr -d com.apple.quarantine /usr/local/bin/said

…then run it again. (Or: System Settings → Privacy & Security → "Open Anyway".)

## Step 4 — Confirm it works

Open a **new** terminal and run:

    said --version

You should see:

    said 0.11.1

- **If you get "command not found" / "not recognized"** → the folder isn't on your PATH
  yet, or you didn't open a new terminal. Re-check Step 3, or run it by full path
  (`C:\said\said.exe --version` on Windows, `~/said/said --version` on macOS/Linux).

## Result

`said` is installed and runs from any folder. You're ready to create your first memory.

## Next step

- **[Tutorial: Your first portable brain](cli/tutorial-your-first-brain.md)** — create a
  brain, store a memory, and ask it a question.
