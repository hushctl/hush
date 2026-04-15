# Open Source Release Evaluation Checklist

Evaluation criteria for public release readiness, synthesized from the [OpenSSF Best Practices Badge](https://www.bestpractices.dev/en/criteria/0), [CFPB Open Source Checklist](https://github.com/cfpb/open-source-checklist), [CHAOSS Metrics](https://chaoss.community/), and [PurpleBooth's readiness gist](https://gist.github.com/PurpleBooth/6f1ba788bf70fb501439).

---

## 1. Identity & Description

| # | Criterion | Required | Status |
|---|---|---|---|
| 1.1 | Repo has a clear, concise description (what it does, who it's for) | MUST | :white_check_mark: |
| 1.2 | README leads with the value proposition, not the tech | MUST | :white_check_mark: |
| 1.3 | README has a quick-start that works in under 5 minutes | MUST | :white_check_mark: |
| 1.4 | README includes screenshot or demo GIF | SHOULD | :warning: placeholder added, needs recording |
| 1.5 | GitHub repo "About" description and topics are set | SHOULD | :white_check_mark: via `gh repo edit` |

## 2. Licensing & Legal

| # | Criterion | Required | Status |
|---|---|---|---|
| 2.1 | LICENSE file exists in repo root with OSI-approved license | MUST | :white_check_mark: |
| 2.2 | License is referenced in README | MUST | :white_check_mark: |
| 2.3 | No third-party code with incompatible licenses | MUST | :white_check_mark: |
| 2.4 | CONTRIBUTING.md states that contributions are under the same license | SHOULD | :white_check_mark: |
| 2.5 | No PII, internal project names, or company-specific references in code | MUST | :white_check_mark: |

## 3. Documentation

| # | Criterion | Required | Status |
|---|---|---|---|
| 3.1 | README covers: install, usage, configuration, architecture | MUST | :white_check_mark: |
| 3.2 | CONTRIBUTING.md with setup, PR process, code review expectations | MUST | :white_check_mark: |
| 3.3 | SECURITY.md with vulnerability reporting process | MUST | :white_check_mark: |
| 3.4 | CLI help text is accurate and complete (`--help`) | MUST | :white_check_mark: |
| 3.5 | CHANGELOG.md or GitHub Releases with version history | SHOULD | :white_check_mark: |
| 3.6 | Architecture/design doc for contributors | SHOULD | :white_check_mark: |

## 4. Security & Privacy

| # | Criterion | Required | Status |
|---|---|---|---|
| 4.1 | No secrets, API keys, or credentials in the repo | MUST | :white_check_mark: |
| 4.2 | .gitignore covers secrets (.env, *.key, *.pem) | MUST | :white_check_mark: |
| 4.3 | Vulnerability reporting process is documented and private | MUST | :white_check_mark: |
| 4.4 | Dependencies audited for known vulnerabilities (`cargo audit`) | MUST | :white_check_mark: |
| 4.5 | PR template includes security checklist | SHOULD | :white_check_mark: |
| 4.6 | Known security limitations are documented honestly | SHOULD | :white_check_mark: |
| 4.7 | Cryptographic practices use standard algorithms (no custom crypto) | MUST | :white_check_mark: |
| 4.8 | TLS enabled by default, no plaintext fallback | MUST | :white_check_mark: |

## 5. Code Quality & Testing

| # | Criterion | Required | Status |
|---|---|---|---|
| 5.1 | Automated test suite exists and is documented | MUST | :white_check_mark: |
| 5.2 | Tests pass on a clean checkout | MUST | :white_check_mark: |
| 5.3 | CI runs on every PR (build + test + lint) | MUST | :white_check_mark: |
| 5.4 | Linter/static analysis enabled (clippy, tsc) | MUST | :white_check_mark: |
| 5.5 | Build from source works on clean machine with documented steps | MUST | :white_check_mark: |
| 5.6 | No compiler errors, minimal warnings | MUST | :white_check_mark: |
| 5.7 | Code formatting is consistent (rustfmt, prettier) | SHOULD | :white_check_mark: |
| 5.8 | Lock files committed (Cargo.lock, package-lock.json) | MUST | :white_check_mark: |

## 6. Repository Hygiene

| # | Criterion | Required | Status |
|---|---|---|---|
| 6.1 | .gitignore covers build artifacts, deps, editor files, OS files | MUST | :white_check_mark: |
| 6.2 | No large binaries tracked in git | MUST | :white_check_mark: |
| 6.3 | No dead code, commented-out blocks, or debug prints in main paths | SHOULD | :white_check_mark: |
| 6.4 | Repo structure is intuitive and documented | MUST | :white_check_mark: |
| 6.5 | No internal/company-specific references in code or docs | MUST | :white_check_mark: |

## 7. Community Readiness

| # | Criterion | Required | Status |
|---|---|---|---|
| 7.1 | Issue templates (bug report + feature request) | MUST | :white_check_mark: |
| 7.2 | PR template with review checklist | MUST | :white_check_mark: |
| 7.3 | CODEOWNERS file for review routing | SHOULD | :white_check_mark: |
| 7.4 | Branch protection on main (require PR, require CI) | SHOULD | :white_check_mark: via `gh api` |
| 7.5 | Code of Conduct | SHOULD | :white_check_mark: |
| 7.6 | Response time expectation set in CONTRIBUTING.md | SHOULD | :white_check_mark: |
| 7.7 | Blank issues disabled (force templates) | SHOULD | :white_check_mark: |
| 7.8 | Security advisories redirect from issue templates | SHOULD | :white_check_mark: |

## 8. Release & Distribution

| # | Criterion | Required | Status |
|---|---|---|---|
| 8.1 | Semantic versioning used | MUST | :white_check_mark: |
| 8.2 | Build instructions work (`make build` / `make install`) | MUST | :white_check_mark: |
| 8.3 | Install works on clean machine following only README | MUST | :white_check_mark: |
| 8.4 | GitHub Releases with tagged versions and release notes | SHOULD | :warning: create after push |
| 8.5 | Pre-built binaries available for target platforms | SHOULD | :warning: CI job needed |
| 8.6 | HTTPS on all project URLs | MUST | :white_check_mark: |

## 9. CI/CD Pipeline

| # | Criterion | Required | Status |
|---|---|---|---|
| 9.1 | CI runs on push to main and on PRs | MUST | :white_check_mark: |
| 9.2 | Build job (compile succeeds) | MUST | :white_check_mark: |
| 9.3 | Test job (tests pass) | MUST | :white_check_mark: |
| 9.4 | Lint job (clippy, tsc) | MUST | :white_check_mark: |
| 9.5 | Security audit job (cargo audit, dependency scan) | SHOULD | :white_check_mark: |
| 9.6 | Formatting check (rustfmt, prettier) | SHOULD | :white_check_mark: |
| 9.7 | Cross-platform CI (macOS + Linux) | SHOULD | :white_check_mark: |
| 9.8 | No secrets or private registries in CI config | MUST | :white_check_mark: |

---

## Scoring

**Legend:** :white_check_mark: Met | :warning: Partial | :x: Not met

### MUST criteria: 34/34 passed
### SHOULD criteria: 19/22 passed (0 not met, 3 partial)

### Overall: 53/56 (95%)

### Remaining items (post-push)

| # | Item | Effort |
|---|---|---|
| 1.4 | Record demo GIF and add to README (placeholder in place) | small — manual recording |
| 8.4 | Create first GitHub Release with tag + notes | small — after push |
| 8.5 | CI job to attach pre-built binaries to releases | medium |

---

## Sources

- [OpenSSF Best Practices Badge — Passing Criteria](https://www.bestpractices.dev/en/criteria/0)
- [CFPB Open Source Checklist](https://github.com/cfpb/open-source-checklist/blob/master/opensource-checklist.md)
- [CHAOSS Metrics — Project Engagement](https://chaoss.community/kb/metrics-model-project-engagement/)
- [PurpleBooth's Open Source Readiness Checklist](https://gist.github.com/PurpleBooth/6f1ba788bf70fb501439)
- [libresource/open-source-checklist](https://github.com/libresource/open-source-checklist)
- [OpenSSF Security Baseline](https://openssf.org/blog/2026/02/25/getting-an-openssf-baseline-badge-with-the-best-practices-badge-system/)
