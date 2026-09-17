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
git switch -c release/v0.2.0
# bump the version, update the changelog, run the release check
# open a pull request into main and merge it, then tag main:
git switch main && git pull
git tag -a v0.2.0 -m "Redrob Query v0.2.0"
git push origin v0.2.0
# bring main's release commit back so develop does not fall behind:
git switch -c chore/sync-main-to-develop main
# open a pull request into develop
```

hotfix도 같은 형태이며, `hotfix/*`를 `develop`이 아니라 `main`에서 떼고 양쪽으로 머지합니다.

저장소를 포크하고, 브랜치를 자기 포크에 push한 뒤 거기서 pull request를 여십시오. 기여하는 데
쓰기 권한은 필요하지 않으며, 포크에서 온 pull request는 저장소 시크릿 없이 CI를 돌립니다.

## 일상 작업

```bash
git switch develop && git pull
git switch -c fix/short-description

npm install
npm run typecheck
npm run test
npm run build

cargo fmt --all
cargo clippy --all-targets -- -D warnings

npm run release:check   # the release validator, before tagging

# open a pull request into develop
```

브라우저 데모는 의도적으로 결정적인 인메모리 샘플 데이터를 씁니다. 데이터베이스 없이, 누구의
자격증명도 없이 제품을 보여주기 위해서입니다. 오늘 찍은 스크린샷이 다음 달에 찍은 것과 같도록
결정적으로 유지하십시오.

### 커밋

제목은 명령형으로 씁니다. 본문에는 _왜_ 를 적고, 로컬에 저장되는 것이나 앱이 쓸 수 있는 범위를
건드리는 변경이면 그 사실을 본문에 밝히십시오.

## 브랜딩

`src/assets/`의 마크는 Redrob Cowork가 함께 배포하는 것과 같은 Redrob 마크이며, 앱 아이콘은
그것으로 생성됩니다. 비슷하게 보이는 그림으로 대체하는 것이 제품에 로고가 두 개 생기는 경로입니다.

## CI가 검사하는 것

`.github/workflows/ci.yml`은 모든 pull request와 `main`, `develop`으로의 push에서 돕니다. 시크릿을
전혀 쓰지 않으므로 포크에서 온 pull request도 여기 브랜치와 완전히 같은 실행을 받습니다. 잡은 둘이고,
둘 다 머지 전에 필수입니다.

| 검사 | 실행하는 것 |
| --- | --- |
| **릴리스 메타데이터, 타입, 테스트, 빌드** | `npm run release:check`, `npm run typecheck`, `npm test`, `npm run build` |
| **포맷, clippy, 테스트** | `npm run rust:fmt`, `npm run rust:clippy` (`-D warnings`), `npm run rust:test` |

알아둘 만한 것은 `npm run release:check`입니다. 여러 파일로 퍼져 있어 조용히 틀어지는 것들을
고정합니다. `package.json`, `Cargo.toml`, `tauri.conf.json`, `package-lock.json`의 버전, Cargo의
`rust-version`과 `rust-toolchain.toml`의 일치, Tauri의 `productName`과 `identifier`, 그리고
`src-tauri/icons/` 아래 모든 PNG가 RGBA인지입니다. 마지막 항목은 겉모습 문제가 아닙니다.
`tauri::generate_context!`는 RGBA가 아닌 아이콘에서 컴파일 타임에 panic하므로, RGB 아이콘 하나면
데스크톱 앱이 빌드되지 않습니다.

`npm run check`는 이 전체를 한 명령으로 로컬에서 돌립니다.
