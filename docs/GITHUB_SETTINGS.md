# GitHub Repository Settings

These settings help keep Atlas Wallet public while still maintainer-controlled.

## General

In **Settings > General**:

- Disable `Allow merge commits` unless you want merge commits.
- Enable `Allow squash merging`.
- Enable `Automatically delete head branches`.
- Disable `Allow auto-merge` until the project has more mature tests.
- Disable `Allow forking` only if you want maximum control. Keeping forks enabled is normal for open source.

## Branch Protection

In **Settings > Branches**, add a branch protection rule for `main`:

- Require a pull request before merging.
- Require approvals: `1`.
- Dismiss stale pull request approvals when new commits are pushed.
- Require review from Code Owners.
- Require status checks to pass before merging.
- Require branches to be up to date before merging.
- Add required checks after the first GitHub Actions run appears:
  - `Windows build`
- Block force pushes.
- Block deletions.
- Do not allow bypassing the above settings unless you intentionally want maintainer emergency access.

## Actions

In **Settings > Actions > General**:

- Allow GitHub Actions.
- Set workflow permissions to `Read repository contents permission` by default.
- Allow GitHub Actions to create and approve pull requests only if you need that later.

## Security

In **Settings > Code security and analysis**:

- Enable Dependabot alerts.
- Enable Dependabot security updates.
- Enable private vulnerability reporting if available.

## Collaborators

In **Settings > Collaborators and teams**:

- Do not add collaborators casually.
- Prefer pull requests from forks.
- Give direct write access only to people you trust to maintain releases and protect family privacy.
