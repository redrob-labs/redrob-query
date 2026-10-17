# 기여 가이드

[English](./CONTRIBUTING.md) · **한국어**

도와주셔서 감사합니다. 이 문서는 이 저장소의 작업 합의입니다. 브랜치를 어떻게 이름 짓고, 머지
전에 무엇이 초록이어야 하며, 실수로 어기기 쉬운 규칙 두 가지를 다룹니다.

## 규칙 두 가지

**1. 이것은 DBeaver 포크가 아니라 처음부터 구현한 코드입니다.** 워크플로는 DBeaver Community를
참고했지만 코드는 그것에서 파생되지 않았습니다. 따라서 DBeaver 소스, DBeaver 브랜딩, 그리고
Eclipse RCP, OSGi, JDBC, Java, JVM 런타임은 이 저장소에 들어오지 않습니다. JVM을 끌어오는 의존성은
세부사항이 아니라 리뷰 중단 사유입니다.

**2. 0.1 버전은 의도적으로 읽기 전용입니다.** 데스크톱 워크스페이스는 보호되어 있습니다.
PostgreSQL, MySQL, SQLite, MongoDB에서 읽기만 하고 쓰지 않습니다. 쓰기 경로를 추가하는 변경은
제품의 약속을 바꾸는 것이므로, diff에만 담지 말고 pull request 제목에 그렇다고 밝혀야 합니다.

로컬 저장에는 별도의 규칙이 있습니다. 쿼리 텍스트와 탭·히스토리 메타데이터는 저장하고, 결과 행,
데이터베이스 자격증명, 프롬프트는 저장하지 않습니다. 쿼리 텍스트 자체에 민감한 리터럴이 담길 수
있는데, 그래서 히스토리 메뉴에 **Stop saving and clear local data** 가 있고 그 opt-out이 재시작
후에도 유지됩니다. 무엇이 새로 포함되는지 밝히지 않은 채 저장 범위를 넓히지 마십시오.

## 브랜치 모델

오래 유지되는 브랜치가 둘입니다. `develop`은 작업이 들어오는 곳이고, `main`은 릴리스된 상태입니다.

- **`develop`** 이 기본 브랜치이자 통합 브랜치입니다. 작업 브랜치는 여기서 떼고, pull request도
  여기로 엽니다. 저장소를 클론하면 `develop`에 있게 됩니다.
- **`main`** 은 릴리스된 상태입니다. `release/*`와 `hotfix/*` 브랜치에서 온 pull request만 받고,
  릴리스 태그는 여기서 뗍니다. 그 외에는 아무것도 여기로 머지되지 않습니다.
- **작업 브랜치** 는 `develop`에서 뗀 `<type>/<short-slug>` 형식입니다. 예를 들어
  `fix/history-restore`, `feat/mongo-explain`. 타입은 `feat`, `fix`, `chore`, `docs`, `test`,
  `refactor`, `perf`입니다.
- **머지는 squash만 합니다**, 그리고 머지 시 브랜치는 삭제됩니다. pull request 하나가 커밋 하나가
  되므로 `git log develop`이 그래프가 아니라 변경 목록으로 읽힙니다. squash 커밋의 본문은 pull
  request 본문이며, 작업 중에 남긴 메시지를 이어 붙인 것이 아닙니다.

두 브랜치는 이 문서가 아니라 GitHub ruleset으로 강제됩니다.

- 직접 push 금지 — 모든 변경은 pull request로 들어옵니다.
- force push 금지, 브랜치 삭제 금지.
- linear history.
- 필수 상태 검사가 통과해야 합니다 — 가장 최신 tip을 기준으로 통과할 필요는 없으므로, 봇 업데이트가
  줄줄이 쌓여도 하나씩 rebase하고 다시 돌릴 필요가 없습니다.
- 머지 전에 리뷰 스레드가 해결되어야 합니다.

ruleset은 pull request가 *어느* 브랜치에서 왔는지를 표현할 수 없으므로, "`release/*`와 `hotfix/*`
만 `main`으로 머지한다"는 이 문서가 담고 리뷰어가 지키는 관례입니다. 그중 한 부분은 기계로
검사됩니다. 릴리스 워크플로는 커밋이 `origin/main`에서 도달 가능하지 않은 태그의 빌드를 거부하므로,
`develop`에서 바로 태그를 떼면 출시되지 않고 실패합니다.

### 릴리스

```bash
git switch develop && git pull
git switch -c release/0.5.0
# Cargo.toml의 버전을 올리고 CHANGELOG 절을 쓰고 게이트를 돌립니다
# develop으로 풀 리퀘스트를 열어 머지하고, develop -> main 풀 리퀘스트(머지 커밋)를 머지한 뒤 main에 태그를 뗍니다:
git switch main && git pull
git tag -a v0.5.0 -m "Redrob Query v0.5.0"
git push origin refs/tags/v0.5.0:refs/tags/v0.5.0
gh workflow run release.yml --ref main -f tag=v0.5.0
# 그다음 main을 develop으로 다시 머지합니다
```

태그는 `Cargo.toml`의 워크스페이스 버전과 같아야 하고 `CHANGELOG.md`에 `## <version>` 절이 있어야
합니다. 모든 플랫폼 레그가 빌드·서명·검증을 마치면 릴리스는 바로 **발행**됩니다 — 승인할 초안이
없습니다. 릴리스 없이 파이프라인만 돌려 보려면 **Release** 워크플로를 `dry_run=true`로 실행하세요.
지정한 ref를 빌드·서명하고 아무것도 발행하지 않습니다.

hotfix도 같은 형태이며, `hotfix/*`를 `develop`이 아니라 `main`에서 떼고 양쪽으로 머지합니다.

저장소를 포크하고, 브랜치를 자기 포크에 push한 뒤 거기서 pull request를 여십시오. 기여하는 데
쓰기 권한은 필요하지 않으며, 포크에서 온 pull request는 저장소 시크릿 없이 CI를 돌립니다.

## 일상 작업

```bash
git switch develop && git pull
git switch -c fix/short-description

cargo fmt --all
cargo clippy -p redrob-query -p redrob-query-core --all-targets -- -D warnings
cargo test -p redrob-query-core
python3 scripts/check-brand.py

cargo run -p redrob-query     # 앱 실행. 내장 Demo SQLite 프로필은 서버가 필요 없습니다

# develop으로 풀 리퀘스트를 엽니다
```

내장 Demo SQLite 프로필은 결정적인 샘플 데이터가 든 인메모리 데이터베이스입니다. 의도적인 것입니다.
서버도, 누구의 자격 증명도 없이 제품을 보여 주기 위해서입니다. 오늘 찍은 스크린샷이 다음 달에 찍은 것과
같도록 결정적으로 유지하세요.

### 커밋

제목은 명령형으로. 본문에는 _왜_ 바꿨는지를 쓰고, 로컬에 저장되는 것이나 앱이 쓸 수 있는 범위가 바뀌면
그 사실을 거기에 적습니다.

## 브랜딩

모든 브랜드 파일(`resources/icons`, `resources/brand`, 설치 프로그램의 `res/`)은
`scripts/build-brand-assets.py`가 디자인 시스템 배포물에서 만들고 `brand/SOURCES.json`에 sha256으로
고정합니다. 비슷하게 그린 그림으로 바꾸는 것이 제품에 로고가 둘 생기는 경로이고,
`scripts/check-brand.py`가 그것을 거부합니다.

## CI가 검사하는 것

`.github/workflows/ci.yml`은 모든 풀 리퀘스트에서 돕니다. 시크릿을 쓰지 않으므로 포크에서 온 풀 리퀘스트도
여기 브랜치와 똑같이 실행됩니다. 작업은 둘입니다.

| 검사 | 실행하는 것 |
| --- | --- |
| **brand assets** | `python3 scripts/check-brand.py` |
| **fmt, clippy, test (profile ci)** | `cargo fmt --all -- --check`, `cargo clippy -p redrob-query -p redrob-query-core --all-targets --locked --profile ci -- -D warnings`, 같은 크레이트의 `cargo test`. Rust 입력이 바뀌지 않았거나 풀 리퀘스트가 `main`을 향하면 건너뜁니다 |

릴리스 워크플로(`.github/workflows/release.yml`)는 브랜드 검사와 `cargo fmt --check`만 다시 돌리고 세
타깃을 빌드·서명·게시합니다.

## 의존성 업데이트

Dependabot **버전 업데이트는 꺼져 있습니다**. 상시로 pull request 대기열을 만들고 각각 CI를 한 번씩
돌려야 했는데, 이 저장소에서는 그 비용이 잡아내는 것보다 컸습니다. 두 가지가 그 자리를 대신합니다.

- **Dependabot 보안 업데이트는 켜져 있습니다.** 알려진 취약점이 있는 의존성에 대해서는 여전히
  pull request가 자동으로 열립니다. 사람을 방해할 가치가 있는 쪽은 이것입니다.
- **advisory 잡이 모든 pull request를 막습니다.** `cargo audit`이 커밋된 lockfile을 검사하므로,
  아무도 알림을 읽지 않아도 취약한 의존성은 머지되지 않습니다. 보안 업데이트는 알림이고, 이것이 통제입니다.

그래서 일상적인 버전 올리기는 의도적으로 합니다. 지금 하는 변경에 필요한 것만 같은 pull request에서
올리고, 이유를 본문에 적으세요. 관련 없는 버전 변경을 기능 브랜치에 쓸어 담지 않습니다.
