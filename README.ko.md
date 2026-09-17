# Redrob Query

[English](./README.md) · **한국어**

Redrob Query는 Rust 1.94, Tauri 2, React 19, TypeScript로 만든 JVM 없는 AI 보조 데이터베이스 작업공간입니다. 0.1 버전은 PostgreSQL, MySQL, SQLite, MongoDB를 위한 가드가 걸린 읽기 전용 데스크톱 작업공간과, 결정적인 인메모리 샘플 데이터만으로 동작하는 브라우저 데모를 제공합니다.

워크플로는 DBeaver Community를 참고했지만 Redrob Query는 처음부터 새로 구현한 것입니다. DBeaver 소스 코드나 브랜딩이 들어 있지 않고, Eclipse RCP·OSGi·JDBC·Java·JVM 런타임도 없습니다.

## 출하 범위

| 영역 | 기능 |
|---|---|
| 프로필 | 사용자 프로필 생성·테스트·편집·연결·연결 해제·삭제. 내장 프로필은 변경 불가 |
| PostgreSQL | 신원을 검증하는 TLS, 메타데이터, 한계가 정해진 읽기 전용 SQL, 네이티브 스칼라 디코딩 |
| MySQL | 신원을 검증하는 TLS, 메타데이터, 한계가 정해진 읽기 전용 SQL, unsigned/decimal/시간/BIT 디코딩 |
| SQLite | `mode=ro`를 쓰는 읽기 전용 파일 프로필, 메타데이터, 가드가 걸린 단일 문장 읽기. 네이티브 데모는 `:memory:` 사용 |
| MongoDB | 표준/SRV 프로필, 별도 `authSource`, 컬렉션, 최대 25개 문서 필드 샘플링, 엄격한 `find`/`aggregate`/explain JSON |
| 결과 | 25/50/100/250행 페이징, 타입이 있는 가상 스크롤 행, 현재 페이지 정렬·필터, 열 표시 설정, 실행 메시지, 눈에 보이는 CSV 내보내기 |
| 쿼리 작업공간 | 연결이 소유하는 SQL/MQL 탭, 엔진별 시작 쿼리, 로컬 데스크톱 복원, 한계가 정해진 실행 이력, 포매팅, 단축키 |
| Redrob AI | 모델 `auto`를 통한 엔진 인식 SQL/MQL 생성과 설명 |
| 브라우저 데모 | PostgreSQL/MySQL/SQLite/Mongo 샘플 워크플로, 로컬 AI 응답, 원자적 인메모리 관계형 편집 리뷰 |

이 릴리스는 SQL Server를 지원하지 않습니다. 연결 UI에 없고, 프로필 검증과 AI 검증 양쪽에서 거부합니다.

데스크톱 쿼리 결과는 **의도적으로** 읽기 전용입니다. 변경 커맨드는 Tauri에 아예 등록되지 않고, 저장된 데스크톱 프로필은 읽기 전용입니다. 브라우저 데모의 편집은 이전 값까지 포함해 배치 전체를 검증한 뒤 프로필별 인메모리 픽스처를 원자적으로 바꾸며, 새로고침하면 사라집니다.

## 구조

```text
React 19 + Monaco + TanStack Virtual
                │
         Zustand 작업공간 상태
                │ DataBridge
          ┌─────┴───────────┐
     DemoBridge         Tauri IPC 허용목록
   (브라우저 메모리)               │
                            redrob-core (Rust)
                       ┌─────────┼──────────┐
                 SQLx 구체 구현   MongoDB   Redrob AI
                 PG/MySQL/SQLite 드라이버  HTTPS 클라이언트
```

[아키텍처](docs/ARCHITECTURE.md), [보안](docs/SECURITY.md), [데모 안내](docs/DEMO.md), [DBeaver 워크플로 대응표](docs/DBEAVER-PORTING-MAP.md)를 함께 보세요.

## 빠른 시작: 브라우저 데모

브라우저 데모는 데이터베이스나 Redrob 서비스에 접속하지 않고, 작업공간을 저장하지도 않습니다.

```bash
npm ci
npm run dev
```

`http://127.0.0.1:1420`을 엽니다. 데모는 연결된 **Acme Warehouse** 샘플 프로필로 시작합니다. 결정적인 메타데이터와 쿼리, 기본 50행 크기에서 두 페이지 분량의 결과, 로컬 AI 응답, CSV 내보내기, 프로필 수명주기 시뮬레이션, 리뷰를 거치는 인메모리 관계형 편집을 지원합니다.

요구 사항: Node.js 22.12+, npm 11+.

## 데스크톱 개발

1. Node.js 22.12+, npm 11+, 그리고 [Tauri v2 사전 요구 사항](https://v2.tauri.app/start/prerequisites/)의 플랫폼 패키지를 설치합니다.
2. rustup으로 Rust를 설치합니다. `rust-toolchain.toml`이 Rust 1.94.0과 rustfmt, Clippy를 고정합니다.
3. 의존성을 설치하고 Tauri를 실행합니다.

```bash
npm ci
npm run tauri dev
```

현재 호스트 플랫폼용 패키지 빌드:

```bash
npm run tauri build
```

Linux에서는 Tauri가 요구하는 WebKitGTK 4.1, JavaScriptCoreGTK 4.1, libsoup 3, librsvg 개발 패키지가 필요합니다. 프런트엔드 프로덕션 빌드와 모든 `redrob-core` 검사는 그 패키지 없이도 돌아가고, `npm run tauri build`만 필요합니다.

`src-tauri/icons/`에 표준 Tauri PNG·ICNS·ICO·iOS·Android 아이콘이 들어 있습니다. 모두 RGBA여야 합니다 — `tauri::generate_context!`가 RGBA가 아닌 아이콘에서 컴파일 타임에 패닉하므로, `npm run release:check`가 이를 단정합니다.

서명된 인스톨러는 **Signed desktop release** 워크플로만 만듭니다. `vMAJOR.MINOR.PATCH` 태그를 푸시하면 Linux x64·macOS Intel·macOS Apple Silicon·Windows x64를 빌드하고, 업데이터 번들에 서명하고, macOS 빌드를 공증한 뒤 `latest.json`을 포함한 전부를 **초안** GitHub Release에 올립니다. 그 초안을 발행하는 것이 곧 릴리스입니다 — GitHub Releases가 업데이트 채널이라 `releases/latest/download/latest.json`은 사람이 발행한 버전만 서비스합니다. 자격 증명이 없으면 서명 없는 산출물을 발행하는 대신 릴리스를 중단합니다.

## 데스크톱 로컬 작업공간 데이터

데스크톱 모드는 열린 쿼리 탭 최대 30개와 첫 페이지가 성공한 실행 이력 50개를 버전이 붙은 렌더러 로컬 스토리지에 저장합니다. 저장되는 것은 쿼리 텍스트와 탭·이력 메타데이터뿐이고, 결과 행·데이터베이스 자격 증명·AI 프롬프트는 절대 저장하지 않습니다. 빈 탭 초안도 유효하며 복원됩니다. 실행 이력은 첫 페이지에서 성공적으로 끝난, 비어 있지 않은 쿼리만 남깁니다. 쿼리 텍스트는 평문이고 그 안에 민감한 리터럴이 들어 있을 수 있으므로, 이력 메뉴에 **저장 중단 및 로컬 데이터 삭제**가 있습니다. 이 opt-out은 별도로 저장되어 재시작 후에도 사용자가 명시적으로 다시 켤 때까지 꺼진 상태로 유지됩니다. 잘못된·중복·엔진 불일치·과대·손상된 항목은 거부하며, 저장이 실패하면 성공했다고 말하지 않고 실패를 표시합니다.

브라우저 데모의 탭과 이력은 현재 페이지 세션 동안만 메모리에 남습니다. 데스크톱 상태를 선택·복원·로드하는 것이 자동 연결이나 자동 쿼리 실행을 일으키는 일은 없습니다.

## 검증

```bash
npm run release:check
npm run typecheck
npm test
npm run test:coverage
npm run build
npm audit --audit-level=low
cargo fmt --all -- --check
cargo clippy --locked -p redrob-core --all-targets --all-features -- -D warnings
cargo test --locked -p redrob-core --all-features
cargo audit --file Cargo.lock
```

`npm run release:check`는 npm·Cargo·Tauri·락파일·Rust 툴체인 버전, 제품명, 번들 식별자, 창 라벨, 개발 URL 메타데이터, 그리고 아이콘이 모두 RGBA인지 검증합니다. 커버리지 하한은 statements/lines 85%, branches 75%, functions 65%입니다.

기본 Rust 코어 실행은 격리된 테스트만 돌리고 외부 커넥터 테스트 3개는 ignored로 보고합니다. 이건 실제 서버에 대한 증거가 아닙니다. 그 게이트를 돌리려면 버려도 되는 서비스 URL을 넣고 명시적으로 opt-in 하세요.

```bash
export REDROB_TEST_POSTGRES_URL='postgresql://…'
export REDROB_TEST_MYSQL_URL='mysql://…'
export REDROB_TEST_MONGO_URL='mongodb://…'
export REDROB_TEST_MONGO_DATABASE='redrob_test' # 선택
cargo test --locked -p redrob-core --all-features -- --ignored
```

명시적으로 실행한 라이브 테스트는 필요한 URL이 없으면 실패합니다. 프로덕션 자격 증명을 쓰지 마세요. Mongo 게이트는 UUID 이름의 컬렉션만 만들고 지웁니다. Rust는 `.env` 파일을 자동으로 읽지 않습니다.

`npm run check`는 워크스페이스 전체 Rust 검사까지 포함하므로 Linux에서는 네이티브 Tauri 시스템 패키지가 필요합니다.

## 보안 요약

- 비밀이 아닌 프로필 메타데이터는 평문 `connections.json`에 저장하고, 비밀번호와 Redrob 키는 OS 키링을 씁니다.
- 비밀을 담지 않는 fsync 저널, 준비 단계 키링 항목, 배타적 프로필 락이 갱신을 복구 가능하게 만들고 안전하지 않은 복구나 동시 소유 상황에서 닫힌 방향으로 실패하게 합니다.
- 데스크톱 관계형 읽기는 보수적으로 읽기 전용으로 분류된 문장 하나만 받습니다. PostgreSQL/MySQL은 읽기 전용 트랜잭션을 추가하고, SQLite 파일 프로필은 `mode=ro`로 열며 닫힌 방향으로 실패하는 PRAGMA 허용목록을 씁니다.
- Mongo 요청은 엄격하고 한계가 있습니다. aggregate의 `$out`과 `$merge`는 재귀적으로 거부합니다. 메타데이터는 최대 25개 문서에서 최상위 타입·존재·널 허용 힌트를 병합한 것이며 완전한 스키마가 아닙니다.
- PostgreSQL/MySQL TLS를 켜면 공개 신뢰 루트로 인증서 신원을 검증합니다. 사설·자체 서명 CA 설정은 제공하지 않습니다.
- 데스크톱 AI는 프롬프트와 선택적으로 현재 쿼리를 Redrob에 보냅니다. 결과 행과 자격 증명은 첨부하지 않지만, 사용자가 그 텍스트에 직접 넣은 민감한 리터럴은 전송됩니다.

전체 [보안 모델과 한계](docs/SECURITY.md)를 읽어 보세요.

## 현재 경계

- 데스크톱·네이티브 결과 변경 없음. 브라우저 데모 편집은 샘플 전용이고 메모리에만 있습니다.
- 페이징은 서버·브리지 단위로 한계가 있고, 정렬과 필터는 현재 로드된 페이지에만 적용됩니다.
- 사설 CA·클라이언트 인증서 UI, SSH 터널, 클라우드 인증 플러그인, 스크립트 실행기, ER 다이어그램, 관리 도구 모음, 데이터 전송 파이프라인, 비교·마이그레이션 도구, 드라이버 마켓플레이스, SQL Server 커넥터는 없습니다.
- Mongo 샘플 메타데이터는 한계가 있는 힌트이며, 컬렉션 스키마를 확정적으로 추론한 것이 아닙니다.
- 외부 PostgreSQL/MySQL/Mongo와 Redrob API 연동은 사용자가 제공하는 서비스·자격 증명이 필요하고, 브라우저 데모는 그것을 검증하지 않습니다.

## Redrob AI 설정

데스크톱 앱의 **Redrob 설정**에서 키를 OS 키링에 저장하세요. 개발 중에는 비어 있지 않은 `REDROB_API_KEY`가 저장된 키보다 우선합니다. 네이티브 클라이언트는 다음을 호출합니다.

```text
https://console.redrob.ai/api/backend/v1/chat/completions
```

프로바이더 요청은 모델 `auto`를 씁니다. 브라우저 데모의 AI는 로컬이고 결정적이며, 데모에 입력한 키는 보관하거나 전송하지 않습니다.

## 기여

[CONTRIBUTING.md](CONTRIBUTING.md)에 브랜치 모델, 필수 검사, 그리고 실수로 깨기 쉬운 두 가지 규칙이 있습니다.

## 라이선스와 귀속

Redrob Query는 [Apache License 2.0](LICENSE)으로 배포됩니다. DBeaver Community 워크플로 귀속은 [NOTICE](NOTICE)를 보세요. 서드파티 구성요소는 각자의 라이선스를 따릅니다.
