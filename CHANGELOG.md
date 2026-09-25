# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Nothing is released yet.

## [Unreleased]

### Added
- Licence, module exception, and `NOTICE` covering the mpv/FFmpeg distribution
  requirements.
- `docs/architecture.md`: layer boundaries, the separate-process player decision, build
  profiles, IPC surface rules, update flow, accessibility gate.
- `.env.example` with no network defaults.

### Deliberately absent
- The Tauri scaffold. It lands with the first real commit, after the mpv IPC prototype
  proves the player integration shape (madde 2). A half-generated scaffold rots and hides
  which decisions are actually made.

[Unreleased]: https://github.com/EON-Extensible-Open-Network/eon-stream-app/compare/main...HEAD
