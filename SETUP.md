# Setup Checklist

After creating a repo from this template, complete these steps.

## Automated (already in the template)

- [x] GitHub Actions CI: test, security, release workflows
- [x] Dependabot: weekly cargo and GitHub Actions updates
- [x] Pre-commit hooks via prek
- [x] cargo-deny configuration
- [x] CHANGELOG scaffold
- [x] CLAUDE.md with development commands

## Search and replace

- [ ] Replace `my-project` with your project name in `Cargo.toml`, `README.md`, and `CLAUDE.md`
- [ ] Replace `OWNER/REPO` with your GitHub path in `Cargo.toml` and `CHANGELOG.md`
- [ ] Replace `Your Name` with your name in `Cargo.toml`
- [ ] Update `description`, `categories`, and `keywords` in `Cargo.toml`
- [ ] Update `README.md` with your project description and usage
- [ ] Update `CLAUDE.md` with project-specific context
- [ ] Strip `<!-- Template users: ... -->` and similar meta-comments from `README.md` and `CLAUDE.md` after the placeholder substitutions

## GitHub Settings (manual)

These settings cannot be configured via code and must be set in the GitHub UI.

### Repository settings

- [ ] **Settings > General > Features:** Enable "Issues" and "Projects" if not already
- [ ] **Settings > Branches > Branch protection:** Add rule for `main`.

  **Apply protection only after the first PR has merged** — otherwise the rules block the very PR that first makes the required checks real.

  PR-triggered checks you can require:

  - `MSRV Check` — cheap; catches accidental use of post-MSRV features
  - `Lint` — fmt, clippy, doctest, cargo doc (ubuntu-only)
  - `Test on ubuntu-latest` — primary CI signal
  - `Test on macos-latest` / `Test on windows-latest` — broader coverage, slower merges
  - `Security Audit`, `License & Dependency Check`, `Supply Chain Review`
  - `CodeQL`

  **Solo-friendly minimum:** `MSRV Check`, `Lint`, `Test on ubuntu-latest`, `Security Audit`, `License & Dependency Check`, `Supply Chain Review`. **Full coverage:** add `Test on macos-latest`, `Test on windows-latest`, and `CodeQL`.

  Either click through the UI, or apply via `gh api` (requires `admin:repo` scope — run `gh auth refresh -s admin:repo` first if needed):

  ```bash
  gh api -X PUT repos/OWNER/REPO/branches/main/protection \
    --input - <<'JSON'
  {
    "required_status_checks": {
      "strict": true,
      "checks": [
        {"context": "MSRV Check"},
        {"context": "Lint"},
        {"context": "Test on ubuntu-latest"},
        {"context": "Security Audit"},
        {"context": "License & Dependency Check"},
        {"context": "Supply Chain Review"}
      ]
    },
    "enforce_admins": false,
    "required_pull_request_reviews": null,
    "restrictions": null,
    "required_linear_history": false,
    "allow_force_pushes": false,
    "allow_deletions": false,
    "required_conversation_resolution": true
  }
  JSON
  ```

### Code security

- [ ] **Settings > Code security and analysis** (`/settings/security_analysis`): Enable:
  - Private vulnerability reporting
  - Dependabot alerts
  - Dependabot security updates
  - Dependabot malware alerts
  - Code scanning (CodeQL) via default setup
  - Secret scanning
  - Push protection

### Secrets (for crates.io publishing)

- [ ] **Settings > Secrets and variables > Actions:** Add `CARGO_REGISTRY_TOKEN`
  - Generate at https://crates.io/settings/tokens
  - Scopes: `publish-new` (required for the first-ever publish) **and** `publish-update` (required for all subsequent releases)
  - Crate name: scope the token to your crate. After the first release, you can regenerate a narrower token with only `publish-update` if you want strict least-privilege.

### Environments (optional)

- [ ] Create `release` environment with required reviewers if you want manual approval before publishing

## Local setup

### Install tools

```bash
cargo install cargo-nextest cargo-audit cargo-deny cargo-vet cargo-mutants cargo-semver-checks prek
```

If any `cargo install` fails with a rustc version mismatch (`requires rustc X.Y.Z or newer, while the currently active rustc version is ...`), run `rustup update stable` — or run `cargo install` from outside any directory containing a `rust-toolchain.toml` override. `cargo install` uses the *active* toolchain, which is directory-scoped when a toolchain file is present in the working tree.

### Enable pre-commit hooks

```bash
prek install --overwrite
```

`--overwrite` replaces any pre-existing `.git/hooks/pre-commit` from the
legacy Python `pre-commit`. Without it, prek keeps the old hook at
`.git/hooks/pre-commit.legacy` and runs both — every check fires twice.

### Verify everything works

```bash
cargo build
cargo nextest run
cargo deny check
```

### Initialize cargo-vet

See the cargo-vet section below.

## cargo-vet initialization

After your first `cargo build`, initialize supply chain auditing:

```bash
cargo vet init
cargo vet import bytecode-alliance
cargo vet import embark-studios
cargo vet import fermyon
cargo vet import google
cargo vet import isrg
cargo vet import mozilla
cargo vet import zcash
```

`cargo vet import <name>` resolves the canonical URL for each known
trusted org — no need to pin a URL. Together, these seven imports
absorb the bulk of common transitive dependencies (in one dogfooding
run on a real project they converted 142 exemptions into real audits
without any maintainer work).

If your crate has no third-party dependencies yet, `cargo vet` passes immediately with nothing to audit. The imports above pre-seed trusted audit sets so that when you add your first dependency, the audits are already in place — you can safely defer the `cargo vet import ...` commands until then.

### Transitively trust publishers your imported orgs trust

Bigger leverage on top of imports. The orgs you've imported have
already certified specific *individual* publishers. `cargo vet trust`
extends those certifications to your tree — no first-hand audit work.

Run `cargo vet suggest` to see candidates — it surfaces publishers
already trusted by 2+ of your imported orgs. High-leverage names to
apply by default (broadly endorsed, ship widely-used crates):

```bash
cargo vet trust --all seanmonstar    # hyper, reqwest, mime_guess, ...
cargo vet trust --all BurntSushi     # regex, regex-syntax, csv, ...
cargo vet trust --all epage          # clap, toml, anstream, ...
cargo vet trust --all kennykerr      # windows-rs, windows-targets, ...
cargo vet trust --all Lokathor       # bytemuck, ...
```

For multi-maintainer flagship packages (clap, tower, windows-*), add
`--allow-multiple-publishers` so a release by any of the maintainers
counts (not just the trusted one):

```bash
cargo vet trust --all epage --allow-multiple-publishers
cargo vet trust --all seanmonstar --allow-multiple-publishers
cargo vet trust --all kennykerr --allow-multiple-publishers
```

In dogfooding on a real project, this took **304 → 237 exemptions**
(a further 22% reduction on top of imports) with zero audit work.

Then exempt any remaining unaudited dependencies:

```bash
cargo vet
# Follow the prompts to exempt crates
```

### Per-PR workflow for unvetted entries

**Never auto-regenerate exemptions in CI.** A Supply Chain Review
failure on a Dependabot PR is the *signal* that new unvetted code is
arriving — pause and decide before merging. Auto-regenerating
rubber-stamps every upstream change and defeats the point of
`cargo vet`.

For each unvetted entry in a failing PR:

- **`cargo vet certify <crate> <version>`** — gold standard. Read the
  diff, attest, commit the new audit onto the PR branch.
- **`cargo vet regenerate exemptions`** — bronze standard. Acceptable
  for patch bumps from known publishers; not for new transitives or
  majors. Commit the config change onto the PR branch.
- **Reject** if the diff looks sketchy.

## Release ceremony (final step — cut vX.Y.Z)

Only run this after every box above is checked and your first feature PR has merged.

1. Promote `## [Unreleased]` in `CHANGELOG.md` to `## [X.Y.Z] - YYYY-MM-DD` (use today's date); leave a fresh `## [Unreleased]` above it for future work.
2. Update the compare links at the bottom of `CHANGELOG.md`:
   - Change `[Unreleased]` to `https://github.com/OWNER/REPO/compare/vX.Y.Z...HEAD`.
   - Add `[X.Y.Z]: https://github.com/OWNER/REPO/releases/tag/vX.Y.Z`.
3. Update the version in `Cargo.toml` to `X.Y.Z`.
4. Remove this file — you won't need it again: `git rm SETUP.md`.
5. Commit, tag, push:
   ```bash
   git add Cargo.toml CHANGELOG.md
   git commit -m "Release vX.Y.Z"
   git tag vX.Y.Z
   git push origin main --tags
   ```
6. Watch the Release workflow. On success, verify end-to-end:
   ```bash
   cargo install <crate> && <crate> --version
   ```
