<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Beekeeper Studio 이식 경계 — 측정으로 정한 결정

**정본 핀**: `c67ca751f4a006b195d6a29d91a9a4f7b11e84b1`
**측정 일자**: 2026-09-30
**측정 스크립트**: `research/beekeeper-coupling.py` (작업 공간)

계획서는 이식 경계를 **확장자**로 그었다 — "TypeScript 3.2 MB는 그대로 이식되고, Vue 1.3 MB는
React로 번역해야 한다". 확장자는 이식 가능성을 결정하지 않는다. **`import` 문이 결정한다.**
Electron IPC를 부르거나 Vuex 스토어를 읽는 `.ts` 파일은 `.vue` 템플릿과 똑같이 호스트
애플리케이션에 묶여 있고, query는 Tauri + React여서 둘 다 없다.

그래서 파일마다 **가장 무거운 결합 하나**로 분류했다. 결합이 둘이면 "절반만 이식 가능"이 아니라
두 번 묶인 것이다.

## 측정 1 — TypeScript는 무엇에 묶여 있는가

`apps/studio/src`, `.d.ts` 제외, **372개 / 1,482 KB**

| 결합 | 파일 | KB | 비율 | 뜻 |
|---|---:|---:|---:|---|
| **portable** | **277** | **813** | **74.5%** | Electron·Vuex·Vue·Node 참조 없음. 그대로 이식된다 |
| vuex | 38 | 180 | 10.2% | 스토어에 묶임. 번역 대상 |
| node | 31 | 339 | 8.3% | `fs`·`path`·`process.env`. 원칙적으로 이식 가능하지만 브라우저 번들에 그대로 못 들어간다 |
| vue | 13 | 86 | 3.5% | `.ts`인데 Vue를 참조한다. 확장자가 거짓말하는 파일들 |
| electron | 13 | 65 | 3.5% | IPC·BrowserWindow. query에는 대응물이 없다 |

계획서의 "3.2 MB"는 테스트(136파일 1,013 KB)·`src-commercial`(51파일)·e2e(43파일)·ui-kit을 모두
합친 수였다. 실제 제품 소스는 1,482 KB이고 그중 **74.5%가 결합 없이 이식된다.**

## 측정 2 — 계획서가 지목한 역할별 결합

| 역할 | portable | node | vue | vuex | electron | 판정 |
|---|---:|---:|---:|---:|---:|---|
| data editor / 쓰기 경로 | 19 | 0 | 3 | 2 | 0 | 대부분 이식 |
| DDL / 제약 / 인덱스 | 35 | 4 | 2 | 0 | 0 | 대부분 이식 |
| **ER 다이어그램** | **0** | **0** | **0** | **0** | **0** | **소스가 없다 — 아래 참조** |
| 다중문 실행기 | 3 | 4 | 0 | 0 | 0 | 이식 |
| 트랜잭션 툴바 | 4 | 4 | 2 | **26** | 0 | **번역** — 유일하게 Vuex가 지배하는 역할 |
| 드라이버 / 방언 | 130 | 9 | 5 | 9 | 2 | 범위 밖 (query는 Rust SQLx) |

두 행이 계획서를 뒤집는다.

**트랜잭션 툴바는 이식이 아니라 번역이다.** 36파일 중 26이 Vuex에 묶여 있다. 계획서가 이 역할을
TypeScript 쪽에 넣어 "그대로 이식"으로 분류한 것은 틀렸다.

**ER 다이어그램은 이식할 소스가 아예 없다.**

## 측정 3 — ER 다이어그램은 소스가 아니라 플러그인이다

공개 트리 전체에서 `entity.relation`, `erd`, `relationship.diagram` 을 검색한 결과 **0건**이다.
`.ts`에도 `.vue`에도 없고, 그래프 레이아웃 라이브러리 import도 없다. 상용 55파일 목록에도 없다
(그쪽은 드라이버와 핸들러다).

실제 위치는 `apps/studio/package.json`의 의존성이다:

```
"@beekeeperstudio/bks-er-diagram": "^1.1.2"
```

그 패키지를 받아 보면 라이브러리가 아니다. `manifest.json` + `dist/index.html`이고,
`manifestVersion: 1`, `minAppVersion: 5.5.0`, 그리고 탭 뷰와 메뉴 항목을 선언한다. **Beekeeper의
플러그인 호스트가 iframe에 띄우는 독립 웹 앱이다.**

| 항목 | 값 |
|---|---|
| 패키지 | `@beekeeperstudio/bks-er-diagram` 1.1.2 |
| 라이선스 | **GPL-3.0** — query(GPL-3.0-or-later)와 양립한다 |
| 저장소 | 별도 공개 레포 `beekeeper-studio/bks-er-diagram` |
| 형태 | 플러그인 (manifest + 사전 빌드된 HTML) |
| 내부 구현 | Vue + `@dagrejs/dagre` |

**마지막 줄이 이 문서에서 가장 중요한 한 줄이다.** ERD 플러그인은 내부적으로 Vue로 만들어졌지만,
**query는 그것을 알 필요가 없다.** 사전 빌드된 HTML을 iframe에 띄우기 때문이다. 계획서가 이 행에
적어 둔 "Vue를 React로 번역"은 **적용되지 않는다.** 번역할 것이 0줄이다.

## 측정 4 — 플러그인 프로토콜은 공개되어 있다

| 항목 | 값 |
|---|---|
| 프로토콜 | iframe `contentWindow` 에 `postMessage` 요청/응답 |
| 공개 문서 | `docs/plugin_development/` 6개 문서, `api-reference.md` **1,272줄** |
| SDK | `@beekeeperstudio/plugin` 1.7.1, **MIT** |
| 호스트 구현 규모 | `apps/studio/src/services/plugin/`, **3,592줄** |
| 호스트 코드의 결합 | 측정 1에서 **portable** — `PluginManager.ts`, `WebPluginLoader.ts`, `WebPluginManager.ts`, `PluginMenuFactories.ts`, `types.ts` 합 약 62 KB |

요청 메서드는 25개 이상이고, ERD가 쓰는 것은 스키마 읽기 계열이다: `getSchemas`, `getTables`,
`getColumns`, `getPrimaryKeys`, `getTableKeys`, `getIncomingKeys`, `getOutgoingKeys`,
`getTableIndexes`, `getViewContext`, `getViewState`/`setViewState`, `setTabTitle`.

**query는 이 전부를 이미 가진 Rust SQLx 계층에서 답할 수 있다.**

## 결정

**플러그인 호스트를 먼저 구현한다. ERD를 개별 기능으로 이식하지 않는다.**

근거는 하나다. 호스트를 구현하면 ERD 하나가 아니라 **그 프로토콜을 쓰는 모든 플러그인**이 동작한다.
ERD를 직접 만들면 ERD 하나만 생기고, 그것도 소스가 없어서 처음부터 써야 한다. 3,592줄짜리 호스트
쪽이 ERD를 백지에서 구현하는 것보다 싸고, 결과가 넓다.

프로토콜이 공개 문서 1,272줄로 규정되어 있고 SDK가 MIT이므로 추측할 것이 없다. 그리고 호스트 코드
자체가 측정 1에서 portable로 분류됐다 — Electron에도 Vuex에도 묶여 있지 않다.

**이 결정이 만드는 새 의존성 두 개**:

| 패키지 | 라이선스 | kind | 이유 |
|---|---|---|---|
| `@beekeeperstudio/plugin` | MIT | library | 플러그인 쪽 SDK. MIT는 GPL-3.0 프로젝트에 들어갈 수 있다 |
| `@beekeeperstudio/bks-er-diagram` | GPL-3.0 | library | ERD 플러그인 자체. GPL-3.0 → GPL-3.0-or-later 한 방향 |

둘 다 커밋이 아니라 `minimum_version`으로 핀한다. 라이브러리이기 때문이다.

## 측정 5 — 그러면 Vue 번역은 얼마나 남는가

**210개 / 1,245 KB** 중 범위 밖을 빼면:

| 구분 | 파일 | KB |
|---|---:|---:|
| 제외: 접속 계층(30) + 상용 유도(7) | 37 | 127 |
| data editor / 쓰기 경로 | 16 | 173 |
| 다중문 실행기 / 에디터 | 9 | 163 |
| DDL / 제약 / 인덱스 | 2 | 19 |
| 사이드바 / 탐색 | 27 | 179 |
| 공통 UI (모달·폼) | 49 | 184 |
| 그 밖 | 70 | 399 |
| **번역 대상** | **173** | **1,117** |

접속 계층 30개가 빠지는 이유는 계획서가 정한 대로다 — query의 접속은 이미 Rust SQLx다. 상용
유도 7개는 받아들일 이유가 없다.

제외 경계는 `components/connection/` **직속 30개**이고, 경로에 `connection`이 들어가는 4개는
여기에 포함되지 않는다. 그 4개는 `components/sidebar/connection/`에 있고 실제로 사이드바
부품이다 — 다만 그중 `AccountStatusButton.vue`와 `NewWorkspaceButton.vue`는 Beekeeper의 계정·
워크스페이스(그들의 클라우드 제공물) 기능이고 query에는 대응물이 없으므로, 사이드바를 실제로
착수할 때 이 둘도 빠진다. 경로 이름으로 뭉뚱그리지 않고 파일 단위로 판정한다.

공통 UI 49개는 번역하지 않을 가능성이 높다. query는 `@redrob-labs/ui` 디자인 시스템을 쓰고, 그것이
모달과 폼을 이미 제공한다. Beekeeper의 모달을 React로 옮기면 디자인 가드가 막는다. 이 49개는
**참조**로 읽고 대응하는 `@redrob-labs/ui` 컴포넌트로 다시 조립하는 것이 맞다. 그 판정은 실제로
착수할 때 파일 단위로 한다.

## 정리 — 계획서가 틀린 곳

| 계획서 | 측정 결과 |
|---|---|
| TypeScript 3.2 MB는 그대로 이식 | 제품 소스는 1,482 KB이고 그중 74.5%가 이식 가능. 나머지 25.5%는 Vuex·Electron·Node·Vue에 묶임 |
| 트랜잭션 툴바는 TS이므로 이식 | 36파일 중 26이 Vuex. **번역이다** |
| ER 다이어그램을 이식 | 이식할 소스가 없다. GPL-3.0 **플러그인 패키지**이고, 플러그인 호스트를 구현하면 얻는다 |
| Vue 1.3 MB를 React로 번역 | ERD 행에는 적용되지 않는다(iframe이므로 0줄). 나머지는 173파일 1,117 KB이고, 공통 UI 49개는 디자인 시스템 때문에 번역이 아니라 재조립이다 |
