
## 크로스빌드 → skills/app + SKILL.md 데이터 연결 - 2026-06-19
- [ ] 빌드 실행 시점 docker 데몬 가동 여부 — DOWN이면 옵션 B(zigbuild) 폴백 여부를 자동 진행할지, 사용자 확인 후 진행할지
- [ ] "모든 os방식" 범위 — 기존 3-OS(macOS universal/Win x64/Linux x64)로 확정. musl/arm64-linux 추가 불필요 가정이 맞는지 최종 확인 필요
- [ ] vworld.sqlite(132MB) 갱신 여부 — 이번 작업은 바이너리 재빌드만 다룸. hjd-db 재적재가 필요한지 별도 판단
