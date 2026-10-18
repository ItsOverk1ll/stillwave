# ADR 0010: Builds: GitHub Actions for every platform

Status: accepted (Phase 0)

## Context

Nobody can build all five platforms locally.

## Decision

CI builds Windows (NSIS per-user and MSI), Linux x64 (deb, rpm, AppImage), Linux arm64 (deb, native `ubuntu-24.04-arm` runner), Android (debug-signed APK) and an unsigned iOS ipa. The native arm64 runner is free only for public repositories. Native hardware testing comes later.

## Consequences

The first CI run is the first time most artifacts exist; failures there are expected and tracked in the final Phase 0 report.
