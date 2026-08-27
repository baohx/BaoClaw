# Changelog

All notable changes to BaoClaw are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow Semantic Versioning.

## [2.1.0] - 2026-08-27

### Added

- Evolution Engine workflows and task management.
- Telegram v2 gateway transport and daemon session support.
- Improved TUI status and abort handling.
- Shared structured logging for Feishu, Telegram, WhatsApp, and IPC.
- Gateway test coverage for message formatting, splitting, and IPC behavior.
- CI typechecking, gateway tests, and gitleaks secret scanning.

### Security

- Fail-closed Telegram and Feishu chat allowlists.
- Owner-only permissions for WhatsApp session credentials.

### Fixed

- Systemd deployment paths and command execution status handling.

## [2.0.0]

### Added

- BaoClaw v2 daemon, gateway, and TUI architecture.
